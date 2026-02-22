use std::sync::OnceLock;
use super::{ButlerInstaller, JreInstaller, PatchDownloader, VersionManager, InstallPolicy, is_game_installed};
use crate::{ProgressPayload, OperationPhase, WeightedProgressTracker, Localization};

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
    pub(crate) localization: std::sync::Arc<Localization>,
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
            localization: std::sync::Arc::new(Localization::new()),
        }
    }

    /// Ensure the game is installed with weighted progress tracking
    pub async fn ensure_installed_with_weighted_progress(
        &self,
        base_dir: &std::path::PathBuf,
        channel: &str,
        target_version: Option<i32>,
        policy: InstallPolicy,
        progress_callback: impl Fn(ProgressPayload) + Send + Sync + 'static,
        cancel_token: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
        localization: &Localization,
    ) -> anyhow::Result<()> {

        // ── Pre-evaluation: determine which phases are actually needed ──────────
        // This ensures total_steps reflects reality instead of always being 7.

        // Is Java already installed?
        let jre_dir = crate::paths::GamePaths::new(base_dir.clone()).jre();
        let jre_already_installed = crate::java::is_jre_installed_at(&jre_dir);

        // Is Butler already installed?
        let butler_path = crate::paths::GamePaths::new(base_dir.clone()).butler();
        let butler_already_installed = butler_path.exists();

        // Is the game already up-to-date?  We do a lightweight version probe here
        // (no network call yet – just local state) so we can skip download+install
        // phases from the step count when the game is current.
        // We use a quick local check: if the "latest" folder exists we assume it
        // might be current; the real version comparison happens in phase_version.
        // We intentionally keep this conservative: if we're unsure, we include the
        // phases so the step count is never *lower* than reality.
        let is_latest_request = target_version.unwrap_or(0) == 0;
        let install_dir_name_hint = if is_latest_request {
            "latest".to_string()
        } else {
            target_version.unwrap_or(0).to_string()
        };
        let game_files_exist =
            is_game_installed(base_dir, channel, &install_dir_name_hint).await;

        // Build the phase list dynamically.
        // "check" is always first; "finalize" is always last.
        let mut phases: Vec<OperationPhase> = Vec::new();

        phases.push(OperationPhase { id: "check".to_string(), weight: 0.02 });

        if !jre_already_installed {
            phases.push(OperationPhase { id: "jre".to_string(), weight: 0.08 });
        }

        if !butler_already_installed {
            phases.push(OperationPhase { id: "butler".to_string(), weight: 0.10 });
        }

        phases.push(OperationPhase { id: "version".to_string(), weight: 0.05 });

        if !game_files_exist {
            // Fresh install: download + install are both needed.
            phases.push(OperationPhase { id: "download".to_string(), weight: 0.60 });
            phases.push(OperationPhase { id: "install".to_string(), weight: 0.20 });
        } else {
            // Game exists: we still might need to patch, but we don't know yet.
            // Include both phases conservatively; they will be skipped internally
            // if the version is already current (phase_download returns early).
            phases.push(OperationPhase { id: "download".to_string(), weight: 0.55 });
            phases.push(OperationPhase { id: "install".to_string(), weight: 0.15 });
        }

        phases.push(OperationPhase { id: "finalize".to_string(), weight: 0.05 });

        let tracker = WeightedProgressTracker::new(progress_callback, phases);

        // Phase 1: Initial checking
        if self.phase_check(&tracker, base_dir, channel, target_version, policy).await? {
            return Ok(());
        }

        // Phase 2: JRE checking and installation
        self.phase_jre(&tracker, base_dir).await?;

        // Phase 3: Butler installation
        let _butler_path = self.phase_butler(&tracker, base_dir, None).await?;

        // Phase 4: Version checking
        let (_version_info, install_dir_name, target_ver_val, start_version, files_exist) =
            self.phase_version(&tracker, base_dir, channel, target_version).await?;

        // If the game is already installed and fully up-to-date, skip download+install.
        // start_version == target_ver_val means local == remote (no patch needed).
        if files_exist && start_version == target_ver_val {
            println!("[PatchAPI] Game is up-to-date (version {}), skipping download+install.", target_ver_val);
            // Advance the tracker through the skipped phases so global_progress reaches 1.0
            WeightedProgressTracker::set_phase(&tracker, "download");
            WeightedProgressTracker::report_simple(&tracker, 1.0, "launcher.status.up_to_date", None);
            WeightedProgressTracker::set_phase(&tracker, "install");
            WeightedProgressTracker::report_simple(&tracker, 1.0, "launcher.status.up_to_date", None);
        } else {
            // Phase 5: Download patch
            let patch_path = self.phase_download(&tracker, channel, start_version, target_ver_val, cancel_token.clone(), localization).await?;

            // Phase 6: Install patch
            if let Err(e) = self.phase_install(&tracker, base_dir, channel, &install_dir_name, &patch_path, cancel_token.clone(), localization).await {
                // Try recovery
                self.handle_installation_failure(&tracker, base_dir, channel, &install_dir_name, target_ver_val, e, cancel_token.clone(), localization).await?;
            }
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