use anyhow::Result;
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool, OnceLock};

use super::{ButlerInstaller, JreInstaller, VersionManager, PatchDownloader};
use crate::game::progress::{WeightedProgressTracker, OperationPhase, ProgressPayload, DownloadStats};

/// Global singleton instance for PatchApiFrontend
static PATCH_API_INSTANCE: OnceLock<PatchApiFrontend> = OnceLock::new();

/// Frontend integration for the new patch API system
/// This provides a high-level interface that replaces the old functions
#[derive(Clone)]
pub struct PatchApiFrontend {
    butler_installer: ButlerInstaller,
    jre_installer: JreInstaller,
    version_manager: VersionManager,
    patch_downloader: PatchDownloader,
}

impl PatchApiFrontend {
    /// Maps phase identifiers to localization keys
    fn get_phase_localization_key(phase: &str) -> &'static str {
        match phase {
            "download" => "launcher.status.downloading",
            "extract" => "launcher.status.extracting", 
            "verify" => "launcher.status.verifying",
            "install" => "launcher.status.installing",
            "prepare" => "launcher.status.preparing",
            "cleanup" => "launcher.status.cleanup",
            "patch" => "launcher.status.patching",
            _ => "launcher.status.working",
        }
    }

    /// Formats step progress as "Step X of Y"
    fn format_step_progress(current_step: Option<usize>, total_steps: Option<usize>) -> String {
        match (current_step, total_steps) {
            (Some(step), Some(total)) => format!("Step {} of {}", step, total),
            (Some(step), None) => format!("Step {}", step),
            (None, Some(total)) => format!("of {} steps", total),
            (None, None) => String::new(),
        }
    }

    /// Gets the global singleton instance
    pub fn get_instance() -> &'static PatchApiFrontend {
        PATCH_API_INSTANCE.get_or_init(|| PatchApiFrontend::new())
    }

    /// Creates a new frontend instance with default providers
    pub fn new() -> Self {
        Self {
            butler_installer: ButlerInstaller::new(),
            jre_installer: JreInstaller::new(),
            version_manager: VersionManager::new(),
            patch_downloader: PatchDownloader::new(),
        }
    }

    /// Installs Butler using the new patch API system
    /// Replaces: crate::game::patcher::install_butler
    pub async fn install_butler(
        &self,
        client: &reqwest::Client,
        base_dir: &PathBuf,
        progress_callback: impl Fn(&str, f64, &str, u64, u64, Option<String>, Option<usize>) + Clone + Send + Sync + 'static,
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> Result<PathBuf> {
        self.butler_installer.install(client, base_dir, progress_callback, cancel_token).await
    }

    /// Downloads and installs JRE using the new patch API system
    /// Replaces: crate::java::download_jre
    pub async fn download_jre(
        &self,
        client: &reqwest::Client,
        base_dir: &PathBuf,
        progress_callback: impl Fn(&str, f64, &str, u64, u64, Option<String>) + Clone + Send + Sync + 'static,
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> Result<()> {
        self.jre_installer.install(client, base_dir, progress_callback, cancel_token).await
    }

    /// Example implementation using the new WeightedProgressTracker system
    /// This demonstrates how to use the unified progress system
    pub async fn ensure_installed_with_weighted_progress(
        &self,
        client: &reqwest::Client,
        base_dir: &PathBuf,
        channel: &str,
        target_version: Option<i32>,
        policy: crate::game::install::InstallPolicy,
        // NEW: Accept the structured reporter instead of raw callback
        reporter: impl Fn(ProgressPayload) + Send + Sync + 'static,
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> Result<(usize, ())> {
        
        // Define our weighted timeline
        let tracker = WeightedProgressTracker::new(reporter, vec![
            OperationPhase { id: "check".to_string(), weight: 2.0 },
            OperationPhase { id: "jre".to_string(), weight: 25.0 },   // JRE download is heavy
            OperationPhase { id: "butler".to_string(), weight: 15.0 },
            OperationPhase { id: "version".to_string(), weight: 2.0 },
            OperationPhase { id: "download".to_string(), weight: 50.0 }, // Patch download (increased weight)
            OperationPhase { id: "install".to_string(), weight: 5.0 },  // Butler apply (reduced weight)
            OperationPhase { id: "finalize".to_string(), weight: 1.0 },
        ]);

        // Phase 1: Checking
        WeightedProgressTracker::set_phase(&tracker, "check");
        WeightedProgressTracker::report(&tracker, 0.0, "launcher.status.checking", vec![], None);

        // Fast offline path - only if game is actually installed
        if policy == crate::game::install::InstallPolicy::OfflineVerify {
            let is_latest = target_version.unwrap_or(0) == 0;
            let install_dir_name = if is_latest { "latest".to_string() } else { target_version.unwrap_or(0).to_string() };
            
            if crate::game::install::is_game_installed(base_dir, channel, &install_dir_name).await {
                WeightedProgressTracker::report(&tracker, 1.0, "launcher.status.verified", vec![], None);
                return Ok((1, ()));
            }
            // Game not installed, continue with normal installation flow
        }

        // Phase 2: JRE
        WeightedProgressTracker::set_phase(&tracker, "jre");
        WeightedProgressTracker::report(&tracker, 0.0, "launcher.status.checking_java", vec![], None);
        
        // Bridge the legacy JRE callback to our new system
        let tracker_clone = tracker.clone();
        let _java_info = crate::java_detection::ensure_java_available(base_dir).await?;

        // Phase 3: Butler
        WeightedProgressTracker::set_phase(&tracker, "butler");
        WeightedProgressTracker::report(&tracker, 0.0, "launcher.status.checking_tools", vec![], None);
        
        // Bridge the legacy Butler callback
        let tracker_clone = tracker.clone();
        let _butler_path = self.butler_installer.install(client, base_dir, 
            move |phase, pct, speed, total, down, eta, step| {
                let stats = DownloadStats {
                    total_bytes: total,
                    downloaded_bytes: down,
                    speed_str: speed.to_string(),
                    eta_str: eta,
                };
                
                // Use phase to select localization key dynamically
                let localization_key = Self::get_phase_localization_key(phase);
                let step_info = Self::format_step_progress(step, Some(3)); // Butler has ~3 steps
                
                // Combine localization key with step info if available
                let message = if !step_info.is_empty() {
                    format!("{} - {}", localization_key, step_info)
                } else {
                    localization_key.to_string()
                };
                
                WeightedProgressTracker::report(&tracker_clone, (pct / 100.0) as f32, &message, vec![], Some(stats));
            }, 
            cancel_token.clone()
        ).await?;

        // Phase 4: Version Check
        WeightedProgressTracker::set_phase(&tracker, "version");
        WeightedProgressTracker::report(&tracker, 0.5, "launcher.status.checking", vec![], None);
        
        let version_info = self.version_manager.get_version_info(base_dir, channel, target_version.unwrap_or(0)).await?;
        
        let user_version = version_info.user_version;
        let remote_version = version_info.latest_remote;
        let is_latest = user_version == 0;
        let install_dir_name = if is_latest { "latest".to_string() } else { user_version.to_string() };
        let target_ver_val = if is_latest { remote_version } else { user_version };
        let files_exist = crate::game::install::is_game_installed(base_dir, channel, &install_dir_name).await;
        let start_version = if is_latest && files_exist { version_info.current_local } else { 0 };

        // Phase 5: Download Patch
        WeightedProgressTracker::set_phase(&tracker, "download");
        
        if !files_exist || start_version < target_ver_val {
            WeightedProgressTracker::report(&tracker, 0.0, "launcher.status.downloading", vec![], None);
            
            // Bridge the patch download callback
            let tracker_clone = tracker.clone();
            let patch_path = self.patch_downloader.download_patch(
                client, base_dir, channel, start_version, target_ver_val, 
                move |phase, pct, speed, total, down, eta, step| {
                    // No pasar estadísticas durante descarga para evitar duplicación con el mensaje
                    let stats = if total > 0 {
                        Some(DownloadStats {
                            total_bytes: total,
                            downloaded_bytes: down,
                            speed_str: speed.to_string(),
                            eta_str: eta,
                        })
                    } else {
                        None
                    };
                    
                    // Use phase to select localization key dynamically
                    let localization_key = Self::get_phase_localization_key(phase);
                    let step_info = Self::format_step_progress(step, Some(5)); // Patch download has ~5 steps
                    
                    // Combine localization key with step info if available
                    let message = if !step_info.is_empty() {
                        format!("{} - {}", localization_key, step_info)
                    } else {
                        localization_key.to_string()
                    };
                    
                    WeightedProgressTracker::report(&tracker_clone, (pct / 100.0) as f32, &message, vec![], stats);
                },
                cancel_token.clone()
            ).await?;

            // Phase 6: Install with enhanced recovery
            WeightedProgressTracker::set_phase(&tracker, "install");
            WeightedProgressTracker::report(&tracker, 0.0, "launcher.status.patching", vec![], None);
            
            let tracker_clone = tracker.clone();
            let install_result = crate::game::patcher::apply_pwr(
                base_dir, channel, &install_dir_name, &patch_path, 
                move |phase, pct, speed, total, down, eta, step| {
                    // Map patcher progress to our progress system with detailed stats
                    let stats = if total > 0 {
                        Some(DownloadStats {
                            total_bytes: total,
                            downloaded_bytes: down,
                            speed_str: speed.to_string(),
                            eta_str: eta,
                        })
                    } else {
                        None
                    };
                    
                    // Use phase to select localization key dynamically
                    let localization_key = Self::get_phase_localization_key(phase);
                    let step_info = Self::format_step_progress(step, Some(4)); // Patch installation has ~4 steps
                    
                    // Combine localization key with step info if available
                    let message = if !step_info.is_empty() {
                        format!("{} - {}", localization_key, step_info)
                    } else {
                        localization_key.to_string()
                    };
                    
                    if pct >= 95.0 {
                        // Verification phase
                        WeightedProgressTracker::report(&tracker_clone, 0.95, "launcher.status.verifying", vec![], stats);
                    } else if pct >= 10.0 {
                        // Main extraction phase
                        WeightedProgressTracker::report(&tracker_clone, (pct / 100.0) as f32 * 0.9, &message, vec![], stats);
                    } else {
                        // Initial setup
                        WeightedProgressTracker::report(&tracker_clone, (pct / 100.0) as f32 * 0.1, &message, vec![], stats);
                    }
                },
                cancel_token.clone()
            ).await;
            
            match install_result {
                Ok(_) => {
                    println!("[INSTALL] Patch application successful");
                }
                Err(e) => {
                    println!("[INSTALL] Patch application failed: {}", e);
                    
                    // ENHANCED: Try fallback to complete download if patch fails
                    WeightedProgressTracker::report(&tracker, 0.5, "launcher.status.recovering", vec![], None);
                    
                    // Clean up corrupted installation
                    let game_dir = crate::game::paths::GamePaths::new(base_dir.clone())
                        .version_dir(channel, &install_dir_name);
                    if game_dir.exists() {
                        println!("[RECOVERY] Removing corrupted installation for fallback");
                        let _ = tokio::fs::remove_dir_all(&game_dir).await;
                    }
                    
                    // Try downloading complete version instead of patch
                    match self.patch_downloader.download_complete_version(
                        client,
                        base_dir,
                        channel,
                        target_version.unwrap_or(0),
                        |phase, pct, status, total, downloaded, eta, step| {
                            // Enhanced fallback download with real progress stats
                            let stats = if total > 0 {
                                Some(DownloadStats {
                                    total_bytes: total,
                                    downloaded_bytes: downloaded,
                                    speed_str: status.to_string(), // status contains speed info here
                                    eta_str: eta,
                                })
                            } else {
                                None
                            };
                            
                            // Use phase to select localization key dynamically
                            let localization_key = Self::get_phase_localization_key(phase);
                            let step_info = Self::format_step_progress(step, Some(3)); // Fallback download has ~3 steps
                            
                            // Combine localization key with step info if available
                            let message = if !step_info.is_empty() {
                                format!("{} - {}", localization_key, step_info)
                            } else {
                                localization_key.to_string()
                            };
                            
                            WeightedProgressTracker::report(&tracker, 0.5 + (pct as f32 / 100.0) * 0.4, &message, vec![], stats);
                        },
                        cancel_token.clone()
                    ).await {
                        Ok(complete_patch_path) => {
                            println!("[RECOVERY] Complete version downloaded, applying...");
                            
                            // Try applying the complete patch
                            let tracker_clone = tracker.clone();
                            crate::game::patcher::apply_pwr(
                                base_dir, channel, &install_dir_name, &complete_patch_path,
                                move |phase, pct, speed, total, down, eta, step| {
                                    // Enhanced fallback patching with detailed stats
                                    let stats = if total > 0 {
                                        Some(DownloadStats {
                                            total_bytes: total,
                                            downloaded_bytes: down,
                                            speed_str: speed.to_string(),
                                            eta_str: eta,
                                        })
                                    } else {
                                        None
                                    };
                                    
                                    // Use phase to select localization key dynamically
                                    let localization_key = Self::get_phase_localization_key(phase);
                                    let step_info = Self::format_step_progress(step, Some(4)); // Fallback patching has ~4 steps
                                    
                                    // Combine localization key with step info if available
                                    let message = if !step_info.is_empty() {
                                        format!("{} - {}", localization_key, step_info)
                                    } else {
                                        localization_key.to_string()
                                    };
                                    
                                    if pct >= 95.0 {
                                        WeightedProgressTracker::report(&tracker_clone, 0.9, "launcher.status.verifying_fallback", vec![], stats);
                                    } else {
                                        WeightedProgressTracker::report(&tracker_clone, 0.5 + (pct as f32 / 100.0) * 0.4, &message, vec![], stats);
                                    }
                                },
                                cancel_token.clone()
                            ).await?;
                            
                            println!("[RECOVERY] Fallback to complete version successful");
                        }
                        Err(fallback_err) => {
                            println!("[RECOVERY] Fallback download also failed: {}", fallback_err);
                            anyhow::bail!("Installation failed: Patch application failed ({}) and fallback download failed ({})", e, fallback_err);
                        }
                    }
                }
            }
        }

        // Phase 7: Final verification and completion
        WeightedProgressTracker::set_phase(&tracker, "finalize");
        WeightedProgressTracker::report(&tracker, 0.95, "launcher.status.finishing_verification", vec![], None);
        
        // Final verification: ensure the game is truly ready
        let is_latest = target_version.unwrap_or(0) == 0;
        let install_dir_name = if is_latest { "latest".to_string() } else { target_version.unwrap_or(0).to_string() };
        
        // Multiple verification attempts
        let mut verification_passed = false;
        for attempt in 1..=3 {
            if crate::game::install::is_game_installed(base_dir, channel, &install_dir_name).await {
                let game_dir = crate::game::paths::GamePaths::new(base_dir.clone())
                    .version_dir(channel, &install_dir_name);
                
                let integrity_checker = crate::game::patch_api::IntegrityChecker::new();
                match integrity_checker.verify_extraction_integrity(&game_dir).await {
                    Ok(_) => {
                        verification_passed = true;
                        println!("[FINAL] Installation verification passed on attempt {}", attempt);
                        break;
                    }
                    Err(e) => {
                        println!("[FINAL] Verification failed on attempt {}: {}", attempt, e);
                        if attempt < 3 {
                            // Wait a bit before retry
                            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                        }
                    }
                }
            } else {
                println!("[FINAL] Game executable not found on attempt {}", attempt);
                if attempt < 3 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
            }
        }
        
        if verification_passed {
            WeightedProgressTracker::report(&tracker, 1.0, "launcher.status.ready", vec![], None);
            println!("[SUCCESS] Installation completed successfully");
        } else {
            anyhow::bail!("Installation verification failed after 3 attempts: Game installation appears corrupted");
        }

        Ok((7, ()))
    }

    /// Gets comprehensive version information
    /// Replaces: crate::game::patcher::get_version_manifest
    pub async fn get_version_info(
        &self,
        base_dir: &PathBuf,
        channel: &str,
        user_version: i32,
    ) -> Result<crate::game::patcher::GameVersionInfo> {
        self.version_manager.get_version_info(base_dir, channel, user_version).await
    }

    /// Finds the latest available game version
    /// Replaces: crate::game::patcher::find_latest_version
    pub async fn find_latest_version(
        &self,
        channel: &str,
        start_hint: Option<i32>,
    ) -> Result<i32> {
        self.version_manager.find_latest_version(channel, start_hint).await
    }

}

impl Default for PatchApiFrontend {
    fn default() -> Self {
        Self::new()
    }
}
