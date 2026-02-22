use std::sync::{Arc, atomic::AtomicBool, Mutex};
use crate::{WeightedProgressTracker, DownloadStats, Localization};
use super::super::PatchApiFrontend;
use super::super::utils;

impl PatchApiFrontend {
    /// Phase 5: Download patch phase
    pub async fn phase_download(
        &self,
        tracker: &Arc<Mutex<WeightedProgressTracker>>,
        channel: &str,
        start_version: i32,
        target_ver_val: i32,
        cancel_token: Option<Arc<AtomicBool>>,
        localization: &Localization,
    ) -> anyhow::Result<std::path::PathBuf> {
        WeightedProgressTracker::set_phase(tracker, "download");
        WeightedProgressTracker::report(
            tracker,
            0.0,
            "launcher.status.downloading",
            vec![],
            None,
        );

        // Bridge the patch download callback
        let tracker_clone = tracker.clone();
        let localization_clone = localization.clone();
        let patch_path = self
            .patch_downloader
            .download_patch(
                channel,
                start_version,
                target_ver_val,
                Arc::new(move |phase: String, pct: f64, speed: String, total: u64, down: u64, eta: Option<String>, step: Option<usize>| {
                    // No pasar estadísticas durante la descarga para evitar duplicación con el mensaje
                    let stats = if total > 0 {
                        Some(DownloadStats {
                            total_bytes: total,
                            downloaded_bytes: down,
                            speed_str: speed.clone(),
                            eta_str: eta,
                        })
                    } else {
                        None
                    };

                    // Use phase to get localized text dynamically
                    let localized_text = utils::get_phase_localization_text(&phase, &localization_clone);
                    let step_info = utils::format_step_progress(step, Some(5)); // Patch download has ~5 steps

                    // Combine localized text with step info if available
                    let message = if !step_info.is_empty() {
                        format!("{} - {}", localized_text, step_info)
                    } else {
                        localized_text
                    };

                    WeightedProgressTracker::report(
                        &tracker_clone,
                        (pct / 100.0) as f32,
                        &message,
                        vec![],
                        stats,
                    );
                }),
                cancel_token.clone()
            )
            .await?;

        Ok(patch_path)
    }

    /// Phase 6: Install patch phase  
    /// Note: This requires patcher functionality - should be implemented by the engine
    pub async fn phase_install(
        &self,
        tracker: &Arc<Mutex<WeightedProgressTracker>>,
        base_dir: &std::path::PathBuf,
        channel: &str,
        install_dir_name: &str,
        patch_path: &std::path::PathBuf,
        cancel_token: Option<Arc<AtomicBool>>,
        localization: &Localization,
    ) -> anyhow::Result<()> {
        WeightedProgressTracker::set_phase(tracker, "install");
        WeightedProgressTracker::report(
            tracker,
            0.0,
            "launcher.status.patching",
            vec![],
            None,
        );

        let tracker_clone = tracker.clone();
        let localization_clone = localization.clone();
        
        // Use the patcher module to apply the patch
        crate::patcher::apply_pwr(
            base_dir,
            channel,
            install_dir_name,
            patch_path,
            Arc::new(move |phase: String, pct: f64, speed: String, total: u64, down: u64, eta: Option<String>, step: Option<usize>| {
                // Map patcher progress to our progress system with detailed stats
                let stats = if total > 0 {
                    Some(DownloadStats {
                        total_bytes: total,
                        downloaded_bytes: down,
                        speed_str: speed.clone(),
                        eta_str: eta,
                    })
                } else {
                    None
                };

                // Use phase to get localized text dynamically
                let localized_text = utils::get_phase_localization_text(&phase, &localization_clone);
                let step_info = utils::format_step_progress(step, Some(4)); // Patch installation has ~4 steps

                // Combine localized text with step info if available
                let message = if !step_info.is_empty() {
                    format!("{} - {}", localized_text, step_info)
                } else {
                    localized_text
                };

                if pct >= 95.0 {
                    // Verification phase
                    WeightedProgressTracker::report(
                        &tracker_clone,
                        0.95,
                        "launcher.status.verifying",
                        vec![],
                        stats,
                    );
                } else if pct >= 10.0 {
                    // Main extraction phase
                    WeightedProgressTracker::report(
                        &tracker_clone,
                        (pct / 100.0) as f32 * 0.9,
                        "launcher.status.extracting", // Clean key, no formatted text
                        vec![], // Arguments if needed (e.g. filename), but not percentage
                        stats,
                    );
                } else {
                    // Initial setup
                    WeightedProgressTracker::report(
                        &tracker_clone,
                        (pct / 100.0) as f32 * 0.1,
                        &message,
                        vec![],
                        stats,
                    );
                }
            }),
            cancel_token.clone(),
            localization,
        )
        .await?;

        println!("[INSTALL] Patch application successful");
        Ok(())
    }
}