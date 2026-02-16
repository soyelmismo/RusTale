use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};

use super::{PatchApiManager, ButlerInstaller, JreInstaller, VersionManager, PatchDownloader, IntegrityChecker};

/// Frontend integration for the new patch API system
/// This provides a high-level interface that replaces the old functions
#[derive(Clone)]
pub struct PatchApiFrontend {
    api_manager: Arc<PatchApiManager>,
    butler_installer: ButlerInstaller,
    jre_installer: JreInstaller,
    version_manager: VersionManager,
    patch_downloader: PatchDownloader,
    integrity_checker: IntegrityChecker,
}

impl PatchApiFrontend {
    /// Creates a new frontend instance with default providers
    pub fn new() -> Self {
        let api_manager = Arc::new(PatchApiManager::new());
        
        Self {
            api_manager: api_manager.clone(),
            butler_installer: ButlerInstaller::new(api_manager.clone()),
            jre_installer: JreInstaller::new(api_manager.clone()),
            version_manager: VersionManager::new(api_manager.clone()),
            patch_downloader: PatchDownloader::new(api_manager.clone()),
            integrity_checker: IntegrityChecker::new(api_manager),
        }
    }

    /// Installs Butler using the new patch API system
    /// Replaces: crate::game::patcher::install_butler
    pub async fn install_butler(
        &self,
        client: &reqwest::Client,
        base_dir: &PathBuf,
        progress_callback: impl Fn(&str, f64, &str, u64, u64, Option<String>, Option<usize>),
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
        progress_callback: impl Fn(&str, f64, &str, u64, u64, Option<String>),
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> Result<()> {
        self.jre_installer.install(client, base_dir, progress_callback, cancel_token).await
    }

    /// Ensures the game is installed and up to date using the new patch API system
    /// Replaces: crate::game::ensure_installed
    pub async fn ensure_installed(
        &self,
        client: &reqwest::Client,
        base_dir: &PathBuf,
        channel: &str,
        target_version: Option<i32>,
        policy: crate::game::install::InstallPolicy,
        progress_callback: impl Fn(&str, f64, &str, u64, u64, Option<String>, Option<usize>),
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> Result<(usize, ())> {
        progress_callback("check", 0.0, "Checking installation...", 0, 0, None, None);

        // --- FAST PATH: Offline Verification ---
        if policy == crate::game::install::InstallPolicy::OfflineVerify {
            return self.offline_verify(base_dir, channel, target_version, &progress_callback).await;
        }

        // --- NETWORK PATH: Full Update/Install ---

        // 1. Ensure JRE is available (only downloads if needed)
        progress_callback("jre", 0.0, "Checking Java installation...", 0, 0, None, Some(1));
        let _java_info = crate::java_detection::ensure_java_available(base_dir).await?;

        // 2. Install Butler if needed
        progress_callback("butler", 0.0, "Checking Butler installation...", 0, 0, None, Some(2));
        let _butler_path = self.install_butler(client, base_dir, &progress_callback, cancel_token.clone()).await?;

        // 3. Find latest version or use target
        progress_callback("version", 0.0, "Checking for game updates...", 0, 0, None, Some(3));

        let version_info = self.version_manager.get_version_info(client, base_dir, channel, target_version.unwrap_or(0)).await?;

        let user_version = version_info.user_version;
        let remote_version = version_info.latest_remote;
        let is_latest = user_version == 0;

        let install_dir_name = if is_latest {
            "latest".to_string()
        } else {
            user_version.to_string()
        };

        let target_ver_val = if is_latest {
            remote_version
        } else {
            user_version
        };

        let files_exist = crate::game::install::is_game_installed(base_dir, channel, &install_dir_name).await;

        // 4. Download and install game
        let start_version = if is_latest && files_exist {
            version_info.current_local
        } else {
            0
        };

        progress_callback("download", 0.0, "Preparing download...", 0, 0, None, Some(4));

        if !files_exist || start_version < target_ver_val {
            // Download patch
            let (patch_path, sig_path) = self.patch_downloader.download_patch_with_signature(
                client,
                base_dir,
                channel,
                start_version,
                target_ver_val,
                &progress_callback,
                cancel_token.clone(),
            ).await?;

            // Verify integrity
            progress_callback("verify", 0.0, "Verifying patch integrity...", 0, 0, None, Some(5));
            let integrity_result = self.integrity_checker.verify_download_integrity(
                &patch_path,
                Some(&sig_path),
                None, // We don't know expected size beforehand
            ).await?;

            if !integrity_result.is_valid() {
                anyhow::bail!("Patch integrity verification failed: {:?}", integrity_result.errors);
            }

            // Apply patch
            progress_callback("install", 0.0, "Installing game files...", 0, 0, None, Some(6));
            crate::game::patcher::apply_pwr(
                base_dir,
                channel,
                &install_dir_name,
                &patch_path,
                &progress_callback,
                cancel_token.clone(),
            ).await?;
        }

        progress_callback("complete", 100.0, "Installation complete", 0, 0, None, Some(7));

        Ok((7, ()))
    }

    /// Gets comprehensive version information
    /// Replaces: crate::game::patcher::get_version_manifest
    pub async fn get_version_info(
        &self,
        client: &reqwest::Client,
        base_dir: &PathBuf,
        channel: &str,
        user_version: i32,
    ) -> Result<crate::game::patcher::GameVersionInfo> {
        self.version_manager.get_version_info(client, base_dir, channel, user_version).await
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

    /// Downloads a specific patch
    pub async fn download_patch(
        &self,
        client: &reqwest::Client,
        base_dir: &PathBuf,
        channel: &str,
        from_version: i32,
        to_version: i32,
        progress_callback: impl Fn(&str, f64, &str, u64, u64, Option<String>, Option<usize>),
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> Result<PathBuf> {
        self.patch_downloader.download_patch(client, base_dir, channel, from_version, to_version, progress_callback, cancel_token).await
    }

    /// Verifies patch integrity
    pub async fn verify_patch_integrity(
        &self,
        patch_path: &PathBuf,
        signature_path: Option<&PathBuf>,
        expected_size: Option<u64>,
    ) -> Result<super::IntegrityResult> {
        self.integrity_checker.verify_download_integrity(patch_path, signature_path, expected_size).await
    }

    /// Gets access to the patch downloader for advanced usage
    pub fn get_patch_downloader(&self) -> &PatchDownloader {
        &self.patch_downloader
    }

    /// Performs offline verification
    async fn offline_verify(
        &self,
        base_dir: &PathBuf,
        channel: &str,
        target_version: Option<i32>,
        progress_callback: &impl Fn(&str, f64, &str, u64, u64, Option<String>, Option<usize>),
    ) -> Result<(usize, ())> {
        let paths = crate::game::paths::GamePaths::new(base_dir.clone());
        
        // For offline verification, we need to check if all components are available locally
        let ver_str = target_version
            .map(|v| v.to_string())
            .unwrap_or_else(|| "latest".to_string());
        let check_ver = if ver_str == "0" { "latest" } else { &ver_str };
        
        // Check if game files exist
        let game_ok = crate::game::install::is_game_installed(base_dir, channel, check_ver).await;
        
        // Check if JRE is available
        let tools_dir = base_dir.join("tools").join("jre");
        let jre_ok = crate::java::is_jre_installed_at(&tools_dir.join("latest"));
        
        // Check if Butler is available
        let butler_ok = paths.butler().exists();
        
        let all_ok = game_ok && jre_ok && butler_ok;
        
        progress_callback("verify", if all_ok { 100.0 } else { 0.0 }, 
                        if all_ok { "All components verified" } else { "Missing components detected" }, 
                        0, 0, None, Some(1));
        
        if all_ok {
            Ok((1, ()))
        } else {
            anyhow::bail!("Offline verification failed: game={}, jre={}, butler={}", game_ok, jre_ok, butler_ok)
        }
    }
}

impl Default for PatchApiFrontend {
    fn default() -> Self {
        Self::new()
    }
}
