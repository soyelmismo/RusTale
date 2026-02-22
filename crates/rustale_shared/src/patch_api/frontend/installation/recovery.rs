use std::sync::{Arc, atomic::AtomicBool, Mutex};
use crate::{WeightedProgressTracker, DownloadStats, Localization, GamePaths};
use super::super::PatchApiFrontend;
use super::super::utils;

impl PatchApiFrontend {
    /// Enhanced recovery logic when patch installation fails
    pub async fn handle_installation_failure(
        &self,
        tracker: &Arc<Mutex<WeightedProgressTracker>>,
        base_dir: &std::path::PathBuf,
        channel: &str,
        install_dir_name: &str,
        target_version: i32,
        original_error: anyhow::Error,
        cancel_token: Option<Arc<AtomicBool>>,
        localization: &Localization,
    ) -> anyhow::Result<()> {
        // 1. CRITICAL ROBUSTNESS: Check cancellation token FIRST
        if let Some(token) = &cancel_token {
            if token.load(std::sync::atomic::Ordering::Relaxed) {
                println!("[INSTALL] Operation was cancelled by user. Skipping recovery.");
                return Err(anyhow::anyhow!("Operation cancelled"));
            }
        }

        println!("[INSTALL] Patch application failed: {}. Initiating recovery...", original_error);

        // ENHANCED: Try fallback to complete download if patch fails
        WeightedProgressTracker::report(
            tracker,
            0.5,
            "launcher.status.recovering", // Ensure this key exists in locales
            vec![],
            None,
        );

        // Clean up corrupted installation
        let game_dir = GamePaths::new(base_dir.clone())
            .version_dir(channel, install_dir_name);
        if game_dir.exists() {
            println!("[RECOVERY] Removing corrupted installation for fallback");
            let _ = tokio::fs::remove_dir_all(&game_dir).await;
        }

        // Try downloading complete patch directly instead of relying on has_complete_version
        println!("[RECOVERY] Attempting fallback to complete patch 0->{}", target_version);
        
        let tracker_clone = tracker.clone();
        match self
            .patch_downloader
            .download_patch(  // Use download_patch instead of download_complete_version
                channel,
                0,  // Start from version 0
                target_version,
                {
                    let tracker_clone = tracker_clone.clone();
                    let localization_clone = localization.clone();
                    Arc::new(move |phase: String, pct: f64, status: String, total: u64, downloaded: u64, eta: Option<String>, step: Option<usize>| {
                        // Enhanced fallback download with real progress stats
                        let stats = if total > 0 {
                            Some(DownloadStats {
                                total_bytes: total,
                                downloaded_bytes: downloaded,
                                speed_str: status.clone(), // status contains speed info here
                                eta_str: eta,
                            })
                        } else {
                            None
                        };

                        // Use phase to get localized text dynamically
                        let localized_text = utils::get_phase_localization_text(&phase, &localization_clone);
                        let step_info = utils::format_step_progress(step, Some(3)); // Fallback download has ~3 steps

                        // Combine localized text with step info if available
                        let message = if !step_info.is_empty() {
                            format!("{} - {}", localized_text, step_info)
                        } else {
                            localized_text
                        };

                        WeightedProgressTracker::report(
                            &tracker_clone,
                            0.5 + (pct as f32 / 100.0) * 0.4,
                            &message,
                            vec![],
                            stats,
                        );
                    })
                },
                cancel_token.clone(),
            )
            .await
        {
            Ok(complete_patch_path) => {
                println!("[RECOVERY] Complete patch downloaded, applying...");

                // Try applying the complete patch
                let tracker_clone = tracker.clone();
                let localization_clone = localization.clone();
                crate::patcher::apply_pwr(
                    base_dir,
                    channel,
                    install_dir_name,
                    &complete_patch_path,
                    Arc::new(move |phase: String, pct: f64, speed: String, total: u64, down: u64, eta: Option<String>, step: Option<usize>| {
                        // Enhanced fallback patching with detailed stats
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
                        let step_info = utils::format_step_progress(step, Some(4)); // Fallback patching has ~4 steps

                        // Combine localized text with step info if available
                        let message = if !step_info.is_empty() {
                            format!("{} - {}", localized_text, step_info)
                        } else {
                            localized_text
                        };

                        if pct >= 95.0 {
                            WeightedProgressTracker::report(
                                &tracker_clone,
                                0.9,
                                "launcher.status.verifying_fallback",
                                vec![],
                                stats,
                            );
                        } else {
                            WeightedProgressTracker::report(
                                &tracker_clone,
                                0.5 + (pct as f32 / 100.0) * 0.4,
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

                println!("[RECOVERY] Fallback to complete patch successful");
                Ok(())
            }
            Err(fallback_err) => {
                println!("[RECOVERY] Fallback download also failed: {}", fallback_err);
                anyhow::bail!(
                    "Installation failed: Patch application failed ({}) and fallback download failed ({})",
                    original_error,
                    fallback_err
                );
            }
        }
    }
}