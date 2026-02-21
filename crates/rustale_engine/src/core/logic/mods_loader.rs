use crate::game::mods::{
    InstalledModMetadata, ModInfo, ModInstallationRequest, delete_mod_completely,
};
use crate::game::mods_api::{ModRepository, SearchResults};
use crate::game::zip_mods::PatchManifest;
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

// Define a trait for the service's output, decoupling it from the engine's Enum
pub trait ModProgressReporter: Send + Sync {
    fn on_progress(&self, phase: String, progress: f32, stats: Option<String>);
    fn on_error(&self, error: String);
    fn on_finished(&self, result_path: String);
}

// Macros para reducir repetición y estandarizar el progreso
macro_rules! send_progress {
    ($reporter:expr, $phase:expr, $progress:expr, $stats:expr) => {
        $reporter.on_progress($phase.to_string(), $progress, $stats);
    };
    ($reporter:expr, $phase:expr, $progress:expr) => {
        send_progress!($reporter, $phase, $progress, None);
    };
}

macro_rules! check_cancellation {
    ($token:expr) => {
        if $token.load(Ordering::Relaxed) {
            return Err(anyhow::anyhow!("Operation cancelled"));
        }
    };
}

pub struct ModsService {
    http_client: rustale_shared::reqwest::Client,
    repo: Box<dyn ModRepository>,
}

/// Centralized ModsService - Single entry point for all mod operations
///
/// This service provides a unified interface for all mod-related operations:
/// - Installation (JAR/ZIP, local/remote, repository-based)
/// - Loading and listing installed mods
/// - Toggling mods and patches
/// - Update checking
///
/// All operations use centralized progress reporting through FromCore::ProgressUpdate
/// and follow the same cancellation pattern with AtomicBool.
///
/// The service routes everything through the repository (ModRepository) and
/// ModInstallationRequest structures, eliminating the need for callbacks or
/// closure-based arguments that are difficult to manage across threads.
impl ModsService {
    pub fn new(client: rustale_shared::reqwest::Client) -> Self {
        Self {
            repo: Box::new(
                crate::game::mods_api::curseforge::CurseForgeRepository::new_with_client(
                    client.clone(),
                ),
            ),
            http_client: client,
        }
    }

    pub async fn search(&self, query: &str, offset: u32, limit: u32) -> Result<SearchResults> {
        let index = if limit > 0 { offset / limit } else { 0 };
        self.repo.search(query, index, limit).await
    }

    /// Installs a mod using the centralized system - this is the main entry point
    /// Handles all mod types (JAR/ZIP, local/remote, repository-based) with unified progress reporting
    pub async fn install_mod(
        &self,
        req: ModInstallationRequest,
        channel: String,
        game_version: u32,
        base_dir: PathBuf,
        reporter: Arc<dyn ModProgressReporter>,
        cancel_token: Arc<AtomicBool>,
    ) -> Result<String> {
        // Check cancellation at start
        check_cancellation!(cancel_token);

        let version_str = if game_version == 0 {
            "latest".to_string()
        } else {
            game_version.to_string()
        };

        // Send initial progress
        send_progress!(reporter, "Starting installation...", 0.0, None);

        // Check cancellation after initial setup
        check_cancellation!(cancel_token);

        // Clone cancel_token once to avoid multiple moves
        let cancel_token_clone = cancel_token.clone();

        // Route to appropriate installation method
        let result = if let Some(file_url) = &req.file_url {
            // Check if it's a local file path or remote URL
            if file_url.starts_with("/") || file_url.starts_with(".") {
                // Local file installation
                let local_path = PathBuf::from(file_url);
                if file_url.ends_with(".zip") {
                    self.install_local_zip(
                        &req,
                        &channel,
                        &version_str,
                        &base_dir,
                        &reporter,
                        cancel_token_clone.clone(),
                        local_path,
                    )
                    .await
                } else {
                    self.install_local_jar(
                        &req,
                        &channel,
                        &version_str,
                        &base_dir,
                        &reporter,
                        cancel_token_clone.clone(),
                        local_path,
                    )
                    .await
                }
            } else {
                // Remote URL installation
                if file_url.ends_with(".zip") {
                    self.install_zip_from_url(
                        &req,
                        &channel,
                        &version_str,
                        &base_dir,
                        &reporter,
                        cancel_token_clone.clone(),
                    )
                    .await
                } else {
                    self.install_jar_from_url(
                        &req,
                        &channel,
                        &version_str,
                        &base_dir,
                        &reporter,
                        cancel_token_clone.clone(),
                    )
                    .await
                }
            }
        } else {
            // Repository-based installation
            self.install_from_repository(
                &req,
                &channel,
                &version_str,
                &base_dir,
                &reporter,
                cancel_token_clone.clone(),
            )
            .await
        };

        // Handle result
        let result_path = match result {
            Ok(path) => {
                reporter.on_finished(path.to_string_lossy().to_string());
                path
            }
            Err(e) => {
                reporter.on_error(e.to_string());
                return Err(e);
            }
        };

        // Send final progress (already handled by on_finished but for legacy logic compatibility)
        send_progress!(reporter, "Installation completed", 1.0, None);

        Ok(result_path.to_string_lossy().to_string())
    }

    /// Gets available versions for a mod
    pub async fn get_versions(
        &self,
        mod_id: &str,
    ) -> Result<Vec<crate::game::mods_api::GenericFile>> {
        self.repo.get_versions(mod_id).await
    }

    /// Loads locally installed mods and patches
    pub async fn load_local_mods(
        &self,
        base_dir: PathBuf,
        channel: String,
        version: String,
    ) -> Result<(Vec<ModInfo>, Vec<PatchManifest>)> {
        let mods = crate::game::mods::list_mods(&base_dir, &channel, &version).await?;
        let paths = crate::game::GamePaths::new(base_dir);
        let patches_dir = paths.core_patches_dir(&channel, &version);

        // Load Zip Patches (Task::spawn_blocking wrapper logic moved here)
        let patches =
            tokio::task::spawn_blocking(move || -> Result<Vec<PatchManifest>, anyhow::Error> {
                crate::game::zip_mods::list_patches(patches_dir).map_err(|e| anyhow::anyhow!(e))
            })
            .await
            .context("Failed to join blocking task for patches")?
            .context("Failed to list patches")?;

        Ok((mods, patches))
    }

    /// Toggles a mod (JAR)
    pub async fn toggle_mod(
        &self,
        mod_info_name: String,
        enabled: bool,
        base_dir: PathBuf,
        channel: String,
        version: String,
    ) -> Result<()> {
        // Load list to find the mod
        let mods = crate::game::mods::list_mods(&base_dir, &channel, &version).await?;
        if let Some(mod_info) = mods
            .iter()
            .find(|m| m.name == mod_info_name || m.file_name == mod_info_name)
        {
            // Only toggle if state differs
            if mod_info.enabled != enabled {
                crate::game::mods::toggle_mod(&base_dir, &channel, &version, mod_info).await?;

                // Update manifest
                if let Some(meta) = &mod_info.metadata {
                    let mut manifest =
                        crate::game::mods::load_manifest(&base_dir, &channel, &version).await;
                    if let Some(entry) = manifest.iter_mut().find(|m| m.mod_id == meta.mod_id) {
                        entry.enabled = enabled;
                        crate::game::mods::save_manifest(&base_dir, &channel, &version, &manifest)
                            .await?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Toggles a ZIP patch
    pub async fn toggle_patch(
        &self,
        patch_id: String,
        enabled: bool,
        base_dir: PathBuf,
        channel: String,
        version: String,
    ) -> Result<()> {
        let paths = crate::game::GamePaths::new(base_dir);

        tokio::task::spawn_blocking(move || -> Result<(), anyhow::Error> {
            if enabled {
                crate::game::zip_mods::enable_patch(&paths, channel, version, &patch_id)
            } else {
                crate::game::zip_mods::disable_patch(&paths, channel, version, &patch_id)
            }
        })
        .await
        .context("Task join error")?
        .context("Toggle patch failed")
    }

    /// Checks for updates of installed mods
    pub async fn check_updates(
        &self,
        installed_mods: Vec<InstalledModMetadata>,
        patches: Vec<crate::game::zip_mods::PatchManifest>,
        game_version: &str,
    ) -> Result<(
        Vec<String>,
        std::collections::HashMap<String, Vec<crate::game::mods_api::GenericFile>>,
    )> {
        let mut updates = Vec::new();
        let mut cached_map = std::collections::HashMap::new();

        // Check JAR mods
        for installed in installed_mods {
            if installed.provider == crate::game::mods_api::ModProvider::CurseForge {
                if let Ok(versions) = self.get_versions(&installed.mod_id).await {
                    let compatible_file: Option<&crate::game::mods_api::GenericFile> =
                        versions.iter().find(|f| {
                            game_version == "latest"
                                || f.game_versions.contains(&game_version.to_string())
                        });

                    if let Some(latest) = compatible_file {
                        if latest.file_id != installed.file_id {
                            updates.push(installed.mod_id.clone());
                        }
                        cached_map.insert(installed.mod_id.clone(), versions);
                    }
                }
            }
        }

        // Check ZIP patches
        for p in patches {
            if let (Some(rid), Some(prov)) = (&p.remote_id, p.provider) {
                if prov == crate::game::mods_api::ModProvider::CurseForge {
                    if let Ok(versions) = self.get_versions(rid).await {
                        let compatible_file: Option<&crate::game::mods_api::GenericFile> =
                            versions.iter().find(|f| {
                                game_version == "latest"
                                    || f.game_versions.contains(&game_version.to_string())
                            });

                        if let Some(latest) = compatible_file {
                            if let Some(current_file_id) = &p.file_id {
                                if latest.file_id != *current_file_id {
                                    updates.push(rid.clone());
                                }
                            }
                            cached_map.insert(rid.clone(), versions);
                        }
                    }
                }
            }
        }

        Ok((updates, cached_map))
    }

    /// Uninstalls a mod (JAR or ZIP patch)
    pub async fn uninstall_mod(
        &self,
        mod_id: String,
        channel: String,
        game_version: u32,
        base_dir: PathBuf,
        reporter: Arc<dyn ModProgressReporter>,
        _cancel_token: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<()> {
        let version_str = if game_version == 0 {
            "latest".to_string()
        } else {
            game_version.to_string()
        };

        // First, try to determine if this is a JAR mod or ZIP patch
        let mods = crate::game::mods::list_mods(&base_dir, &channel, &version_str).await?;
        let is_jar_mod = mods
            .iter()
            .any(|m| m.name == mod_id || m.file_name == mod_id);

        if is_jar_mod {
            // This is a JAR mod - use complete deletion
            let mod_name = mods
                .iter()
                .find(|m| m.name == mod_id || m.file_name == mod_id)
                .map(|m| &m.file_name)
                .unwrap_or(&mod_id);

            delete_mod_completely(&base_dir, &channel, &version_str, mod_name).await?;
            println!("[ModsService] Uninstalled JAR mod: {}", mod_name);
        } else {
            // This might be a ZIP patch - try to uninstall as patch
            let paths = crate::game::GamePaths::new(base_dir.clone());
            if let Err(e) = crate::game::zip_mods::uninstall_patch(
                &paths,
                channel.clone(),
                version_str.clone(),
                &mod_id,
            ) {
                return Err(anyhow::anyhow!(
                    "Failed to uninstall mod '{}': {}",
                    mod_id,
                    e
                ));
            }
            println!("[ModsService] Uninstalled ZIP patch: {}", mod_id);
        }

        // Send progress update
        reporter.on_progress(
            format!("Mod '{}' uninstalled successfully", mod_id),
            1.0,
            None,
        );
        reporter.on_finished(mod_id.clone());

        Ok(())
    }

    // === Private helper methods for installation ===

    /// Installs a local ZIP file
    async fn install_local_zip(
        &self,
        req: &ModInstallationRequest,
        channel: &str,
        version_str: &str,
        base_dir: &PathBuf,
        reporter: &Arc<dyn ModProgressReporter>,
        cancel_token: Arc<AtomicBool>,
        local_path: PathBuf,
    ) -> Result<PathBuf> {
        send_progress!(reporter, "Installing local ZIP patch...", 0.1, None);

        // Check if file exists
        if !local_path.exists() {
            return Err(anyhow::anyhow!("Local file not found: {:?}", local_path));
        }

        check_cancellation!(cancel_token);

        send_progress!(reporter, "Installing ZIP patch...", 0.7, None);

        // Install the ZIP patch
        let paths = crate::game::GamePaths::new(base_dir.clone());
        let zip_path = local_path.clone();
        let channel = channel.to_string();
        let version_str = version_str.to_string();
        let req = req.clone();
        let paths_clone = paths.clone();

        tokio::task::spawn_blocking(move || -> Result<(), anyhow::Error> {
            crate::game::zip_mods::install_new_patch(
                zip_path,
                &paths_clone,
                channel,
                version_str,
                req,
                Some(cancel_token.clone()),
            )
        })
        .await
        .context("Task join error")?
        .context("ZIP patch installation failed")?;

        Ok(local_path)
    }

    /// Installs a local JAR file
    async fn install_local_jar(
        &self,
        req: &ModInstallationRequest,
        channel: &str,
        version_str: &str,
        base_dir: &PathBuf,
        reporter: &Arc<dyn ModProgressReporter>,
        cancel_token: Arc<AtomicBool>,
        local_path: PathBuf,
    ) -> Result<PathBuf> {
        send_progress!(reporter, "Installing local JAR mod...", 0.1, None);

        // Check if file exists
        if !local_path.exists() {
            return Err(anyhow::anyhow!("Local file not found: {:?}", local_path));
        }

        check_cancellation!(cancel_token);

        send_progress!(reporter, "Installing JAR mod...", 0.8, None);

        // Install the JAR mod
        let paths = crate::game::GamePaths::new(base_dir.clone());
        let mods_dir = paths.mods_dir(channel, version_str);

        let file_name = local_path
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("Invalid file path"))?;

        let target_path = mods_dir.join(file_name);
        tokio::fs::copy(&local_path, &target_path)
            .await
            .context("Failed to copy JAR file to mods directory")?;

        // Save metadata to manifest using the request information
        let metadata = InstalledModMetadata {
            file_name: file_name.to_string_lossy().to_string(),
            mod_name: req.mod_name.clone(),
            provider: req
                .provider
                .unwrap_or(crate::game::mods_api::ModProvider::Local),
            mod_id: req.mod_id.clone(),
            file_id: req.file_id.clone().unwrap_or_else(|| "local".to_string()),
            enabled: true,
            summary: req.summary.clone(),
            logo_url: req.logo_url.clone(),
            install_date: chrono::Utc::now(),
            update_available: None,
        };

        // Load existing manifest and add this mod
        let mut manifest = crate::game::mods::load_manifest(base_dir, channel, version_str).await;
        manifest.push(metadata);
        crate::game::mods::save_manifest(base_dir, channel, version_str, &manifest)
            .await
            .context("Failed to save mod manifest")?;

        Ok(target_path)
    }

    /// Downloads and installs a ZIP mod from URL
    async fn install_zip_from_url(
        &self,
        req: &ModInstallationRequest,
        channel: &str,
        version_str: &str,
        base_dir: &PathBuf,
        reporter: &Arc<dyn ModProgressReporter>,
        cancel_token: Arc<AtomicBool>,
    ) -> Result<PathBuf> {
        send_progress!(reporter, "Downloading ZIP file...", 0.1, None);

        // Clone cancel_token before passing to download function
        let cancel_token_clone = cancel_token.clone();

        // Download file
        let download_result = self
            .download_file_with_progress(
                req.file_url
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("No file URL provided"))?,
                base_dir,
                reporter,
                cancel_token_clone,
                0.1,
                0.7,
            )
            .await?;

        check_cancellation!(cancel_token);

        send_progress!(reporter, "Installing ZIP patch...", 0.7, None);

        // Install the ZIP patch
        let paths = crate::game::GamePaths::new(base_dir.clone());
        let zip_path = download_result.clone();
        let channel = channel.to_string();
        let version_str = version_str.to_string();
        let req = req.clone();
        let paths_clone = paths.clone();

        tokio::task::spawn_blocking(move || -> Result<(), anyhow::Error> {
            crate::game::zip_mods::install_new_patch(
                zip_path,
                &paths_clone,
                channel,
                version_str,
                req,
                Some(cancel_token.clone()),
            )
        })
        .await
        .context("Task join error")?
        .context("ZIP patch installation failed")?;

        Ok(download_result)
    }

    /// Downloads and installs a JAR mod from URL
    async fn install_jar_from_url(
        &self,
        req: &ModInstallationRequest,
        channel: &str,
        version_str: &str,
        base_dir: &PathBuf,
        reporter: &Arc<dyn ModProgressReporter>,
        cancel_token: Arc<AtomicBool>,
    ) -> Result<PathBuf> {
        send_progress!(reporter, "Downloading JAR file...", 0.1, None);

        // Clone cancel_token before passing to download function
        let cancel_token_clone = cancel_token.clone();

        // Download the file
        let download_result = self
            .download_file_with_progress(
                req.file_url
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("No file URL provided"))?,
                base_dir,
                reporter,
                cancel_token_clone,
                0.1,
                0.8,
            )
            .await?;

        check_cancellation!(cancel_token);

        send_progress!(reporter, "Installing JAR mod...", 0.8, None);

        // Install the JAR mod
        let paths = crate::game::GamePaths::new(base_dir.clone());
        let mods_dir = paths.mods_dir(channel, version_str);

        let file_name = download_result.file_name().unwrap();
        let target_path = mods_dir.join(file_name);
        tokio::fs::copy(&download_result, &target_path)
            .await
            .context("Failed to copy JAR file to mods directory")?;

        // Save metadata to manifest so the mod appears in installed_ids after refresh
        let metadata = InstalledModMetadata {
            file_name: file_name.to_string_lossy().to_string(),
            mod_name: req.mod_name.clone(),
            provider: req
                .provider
                .unwrap_or(crate::game::mods_api::ModProvider::Local),
            mod_id: req.mod_id.clone(),
            file_id: req.file_id.clone().unwrap_or_else(|| "unknown".to_string()),
            enabled: true,
            summary: req.summary.clone(),
            logo_url: req.logo_url.clone(),
            install_date: chrono::Utc::now(),
            update_available: None,
        };

        let mut manifest = crate::game::mods::load_manifest(base_dir, channel, version_str).await;
        // Remove any old entry for this mod_id before appending
        manifest.retain(|m| m.mod_id != metadata.mod_id);
        manifest.push(metadata);
        crate::game::mods::save_manifest(base_dir, channel, version_str, &manifest)
            .await
            .context("Failed to save mod manifest after JAR install")?;

        Ok(target_path)
    }

    /// Downloads and installs a mod from repository (no direct URL)
    async fn install_from_repository(
        &self,
        req: &ModInstallationRequest,
        channel: &str,
        version_str: &str,
        base_dir: &PathBuf,
        reporter: &Arc<dyn ModProgressReporter>,
        cancel_token: Arc<AtomicBool>,
    ) -> Result<PathBuf> {
        send_progress!(reporter, "Fetching mod information...", 0.05, None);

        // Get mod versions to find download URL
        let versions = self
            .repo
            .get_versions(&req.mod_id)
            .await
            .context("Failed to fetch mod versions")?;

        // Find the specific file or use latest
        let target_file = if let Some(file_id) = &req.file_id {
            versions
                .iter()
                .find(|f| &f.file_id == file_id)
                .ok_or_else(|| anyhow::anyhow!("File ID {} not found", file_id))?
        } else {
            versions
                .first()
                .ok_or_else(|| anyhow::anyhow!("No files available for mod {}", req.mod_id))?
        };

        check_cancellation!(cancel_token);

        // Create a new request with the file URL
        let mut req_with_url = req.clone();
        req_with_url.file_url = target_file.download_url.clone();

        // Proceed with URL-based installation
        if let Some(ref url) = target_file.download_url {
            if url.ends_with(".zip") {
                self.install_zip_from_url(
                    &req_with_url,
                    channel,
                    version_str,
                    base_dir,
                    reporter,
                    cancel_token,
                )
                .await
            } else {
                self.install_jar_from_url(
                    &req_with_url,
                    channel,
                    version_str,
                    base_dir,
                    reporter,
                    cancel_token,
                )
                .await
            }
        } else {
            Err(anyhow::anyhow!(
                "No download URL available for mod {}",
                req.mod_id
            ))
        }
    }

    /// Downloads a file with progress reporting
    async fn download_file_with_progress(
        &self,
        url: &str,
        base_dir: &PathBuf,
        reporter: &Arc<dyn ModProgressReporter>,
        cancel_token: Arc<AtomicBool>,
        start_progress: f32,
        end_progress: f32,
    ) -> Result<PathBuf> {
        // Create temp directory if needed
        let temp_dir = base_dir.join("temp");
        tokio::fs::create_dir_all(&temp_dir)
            .await
            .context("Failed to create temp directory")?;

        // Get filename from URL
        let filename = url
            .split('/')
            .last()
            .ok_or_else(|| anyhow::anyhow!("Invalid URL: cannot extract filename"))?;
        let file_path = temp_dir.join(filename);

        // Start download
        let response = self
            .http_client
            .get(url)
            .send()
            .await
            .context("Failed to start download")?;

        let total_size = response.content_length().unwrap_or(0);
        let mut downloaded = 0u64;
        let mut stream = response.bytes_stream();
        let mut file = tokio::fs::File::create(&file_path)
            .await
            .context("Failed to create file")?;

        use futures::TryStreamExt;
        use tokio::io::AsyncWriteExt;

        while let Some(chunk) = stream.try_next().await.context("Failed to read stream")? {
            check_cancellation!(cancel_token);
            downloaded += chunk.len() as u64;

            file.write_all(&chunk)
                .await
                .context("Failed to write chunk")?;

            // Report progress directly without spawning
            if total_size > 0 {
                let progress = start_progress
                    + (end_progress - start_progress) * (downloaded as f32 / total_size as f32);
                let progress_msg = format!("Downloading... ({:.0}%)", progress * 100.0);
                let stats_msg = format!(
                    "{} / {} MB",
                    downloaded / 1024 / 1024,
                    total_size / 1024 / 1024
                );

                send_progress!(reporter, progress_msg, progress, Some(stats_msg));
            }
        }

        file.flush().await.context("Failed to flush file")?;

        Ok(file_path)
    }
}
