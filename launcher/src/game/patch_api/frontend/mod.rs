use std::sync::OnceLock;
use super::{ButlerInstaller, JreInstaller, PatchDownloader, VersionManager};
use crate::game::progress::{ProgressPayload, OperationPhase, WeightedProgressTracker};

pub mod components;
pub mod installation;
pub mod query;
pub mod utils;

/// Global singleton instance for PatchApiFrontend
static PATCH_API_INSTANCE: OnceLock<PatchApiFrontend> = OnceLock::new();

/// Frontend integration for the new patch API system
/// This provides a high-level interface that replaces the old functions
#[derive(Clone)]
pub struct PatchApiFrontend {
    pub(crate) butler_installer: ButlerInstaller,
    pub(crate) jre_installer: JreInstaller,
    pub(crate) version_manager: VersionManager,
    pub(crate) patch_downloader: PatchDownloader,
}

impl PatchApiFrontend {
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

    /// Ensure the game is installed with weighted progress tracking
    pub async fn ensure_installed_with_weighted_progress(
        &self,
        client: &reqwest::Client,
        base_dir: &std::path::PathBuf,
        channel: &str,
        target_version: Option<i32>,
        policy: crate::game::install::InstallPolicy,
        progress_callback: impl Fn(ProgressPayload) + Send + Sync + 'static,
        cancel_token: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
        localization: &crate::lang::Localization,
    ) -> anyhow::Result<()> {
        
        // Define phases with their weights
        let phases = vec![
            OperationPhase { id: "check".to_string(), weight: 0.1 },
            OperationPhase { id: "jre".to_string(), weight: 0.1 },
            OperationPhase { id: "butler".to_string(), weight: 0.1 },
            OperationPhase { id: "version".to_string(), weight: 0.1 },
            OperationPhase { id: "download".to_string(), weight: 0.3 },
            OperationPhase { id: "install".to_string(), weight: 0.25 },
            OperationPhase { id: "finalize".to_string(), weight: 0.05 },
        ];
        
        let tracker = WeightedProgressTracker::new(progress_callback, phases);
        
        // Phase 1: Initial checking
        if self.phase_check(&tracker, base_dir, channel, target_version, policy).await? {
            return Ok(());
        }
        
        // Phase 2: JRE checking and installation
        self.phase_jre(&tracker, base_dir).await?;
        
        // Phase 3: Butler installation
        let _butler_path = self.phase_butler(&tracker, client, base_dir, None).await?;
        
        // Phase 4: Version checking
        let (_version_info, install_dir_name, target_ver_val, start_version, _files_exist) = 
            self.phase_version(&tracker, base_dir, channel, target_version).await?;
        
        // Phase 5: Download patch
        let patch_path = self.phase_download(&tracker, client, channel, start_version, target_ver_val, cancel_token.clone(), localization).await?;
        
        // Phase 6: Install patch
        if let Err(e) = self.phase_install(&tracker, base_dir, channel, &install_dir_name, &patch_path, cancel_token.clone(), localization).await {
            // Try recovery
            self.handle_installation_failure(&tracker, client, base_dir, channel, &install_dir_name, target_ver_val, e, cancel_token.clone(), localization).await?;
        }
        
        // Phase 7: Final verification
        self.phase_finalize(&tracker, base_dir, channel, target_version, target_ver_val).await?;
        
        Ok(())
    }
}

impl Default for PatchApiFrontend {
    fn default() -> Self {
        Self::new()
    }
}
