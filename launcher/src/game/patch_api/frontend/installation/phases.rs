use std::sync::{Arc, atomic::AtomicBool, Mutex};
use crate::game::progress::{WeightedProgressTracker, DownloadStats};
use crate::game::patch_api::frontend::PatchApiFrontend;

impl PatchApiFrontend {
    /// Phase 1: Initial checking phase
    pub async fn phase_check(
        &self,
        tracker: &Arc<Mutex<WeightedProgressTracker>>,
        base_dir: &std::path::PathBuf,
        channel: &str,
        target_version: Option<i32>,
        policy: crate::game::install::InstallPolicy,
    ) -> anyhow::Result<bool> {
        WeightedProgressTracker::set_phase(tracker, "check");
        WeightedProgressTracker::report_simple(tracker, 0.0, "launcher.status.checking", None);

        // Fast offline path - only if game is actually installed
        if policy == crate::game::install::InstallPolicy::OfflineVerify {
            let is_latest = target_version.unwrap_or(0) == 0;
            let install_dir_name = if is_latest {
                "latest".to_string()
            } else {
                target_version.unwrap_or(0).to_string()
            };

            if crate::game::install::is_game_installed(base_dir, channel, &install_dir_name).await {
                WeightedProgressTracker::report(
                    tracker,
                    1.0,
                    "launcher.status.verified",
                    vec![],
                    None,
                );
                return Ok(true);
            }
            // Game not installed, continue with normal installation flow
        }
        Ok(false)
    }

    /// Phase 2: JRE checking and installation
    pub async fn phase_jre(
        &self,
        tracker: &Arc<Mutex<WeightedProgressTracker>>,
        base_dir: &std::path::PathBuf,
    ) -> anyhow::Result<()> {
        WeightedProgressTracker::set_phase(tracker, "jre");
        WeightedProgressTracker::report(
            tracker,
            0.0,
            "launcher.status.checking_java",
            vec![],
            None,
        );

        // Bridge the legacy JRE callback to our new system
        let java_info = crate::java_detection::ensure_java_available(base_dir).await?;
        println!("[JRE] Detected Java version: {}", java_info.version);
        Ok(())
    }

    /// Phase 3: Butler installation
    pub async fn phase_butler(
        &self,
        tracker: &Arc<Mutex<WeightedProgressTracker>>,
        client: &reqwest::Client,
        base_dir: &std::path::PathBuf,
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> anyhow::Result<std::path::PathBuf> {
        WeightedProgressTracker::set_phase(tracker, "butler");
        WeightedProgressTracker::report(
            tracker,
            0.0,
            "launcher.status.checking_tools",
            vec![],
            None,
        );

        // Bridge the legacy Butler callback
        let tracker_clone = tracker.clone();
        let progress_cb = Arc::new(move |_phase: &str, pct: f64, status: &str, total: u64, downloaded: u64, eta: Option<String>, _step: Option<usize>| {
            WeightedProgressTracker::report(
                &tracker_clone,
                (pct / 100.0) as f32,
                status,
                vec![],
                if total > 0 {
                    Some(DownloadStats {
                        total_bytes: total,
                        downloaded_bytes: downloaded,
                        speed_str: String::new(),
                        eta_str: eta,
                    })
                } else {
                    None
                },
            );
        });
        
        let butler_path = self
            .butler_installer
            .install(
                client,
                base_dir,
                progress_cb,
                cancel_token,
            )
            .await?;

        println!("[Butler] Binary verified at: {:?}", butler_path);
        Ok(butler_path)
    }

    /// Phase 4: Version checking and information gathering
    pub async fn phase_version(
        &self,
        tracker: &Arc<Mutex<WeightedProgressTracker>>,
        base_dir: &std::path::PathBuf,
        channel: &str,
        target_version: Option<i32>,
    ) -> anyhow::Result<(crate::game::patcher::GameVersionInfo, String, i32, i32, bool)> {
        WeightedProgressTracker::set_phase(tracker, "version");
        WeightedProgressTracker::report_simple(tracker, 0.5, "launcher.status.checking", None);

        let version_info = self
            .version_manager
            .get_version_info(base_dir, channel, target_version.unwrap_or(0))
            .await?;

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
        let files_exist =
            crate::game::install::is_game_installed(base_dir, channel, &install_dir_name).await;
        let start_version = if is_latest && files_exist {
            version_info.current_local
        } else {
            0
        };

        Ok((version_info, install_dir_name, target_ver_val, start_version, files_exist))
    }
}
