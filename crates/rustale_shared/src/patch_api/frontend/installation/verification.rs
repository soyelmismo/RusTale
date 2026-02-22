use std::sync::{Arc, Mutex};
use crate::{WeightedProgressTracker, GamePaths};
use super::super::PatchApiFrontend;
use crate::patch_api::{is_game_installed, save_local_version, IntegrityChecker};

impl PatchApiFrontend {
    /// Phase 7: Final verification and completion
    pub async fn phase_finalize(
        &self,
        tracker: &Arc<Mutex<WeightedProgressTracker>>,
        base_dir: &std::path::PathBuf,
        channel: &str,
        target_version: Option<i32>,
        target_ver_val: i32,
    ) -> anyhow::Result<()> {
        WeightedProgressTracker::set_phase(tracker, "finalize");
        WeightedProgressTracker::report(
            tracker,
            0.95,
            "launcher.status.finishing_verification",
            vec![],
            None,
        );

        // Final verification: ensure the game is truly ready
        let is_latest = target_version.unwrap_or(0) == 0;
        let install_dir_name = if is_latest {
            "latest".to_string()
        } else {
            target_version.unwrap_or(0).to_string()
        };

        // Multiple verification attempts
        let mut verification_passed = false;
        for attempt in 1..=3 {
            if is_game_installed(base_dir, channel, &install_dir_name).await {
                let game_dir = GamePaths::new(base_dir.clone())
                    .version_dir(channel, &install_dir_name);

                let integrity_checker = IntegrityChecker::new();
                match integrity_checker
                    .verify_extraction_integrity(&game_dir)
                    .await
                {
                    Ok(_) => {
                        verification_passed = true;
                        println!(
                            "[FINAL] Installation verification passed on attempt {}",
                            attempt
                        );
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
            WeightedProgressTracker::report_simple(tracker, 1.0, "launcher.status.ready", None);
            println!("[SUCCESS] Installation completed successfully");
            
            // Save version.json only for latest installations and only after successful verification
            if is_latest {
                if let Err(e) = save_local_version(
                    base_dir,
                    channel,
                    target_ver_val,
                )
                .await
                {
                    println!("[WARNING] Failed to save version.json: {}", e);
                } else {
                    println!(
                        "[INFO] Saved version.json for latest version: {}",
                        target_ver_val
                    );
                }
            }
            
            Ok(())
        } else {
            anyhow::bail!(
                "Installation verification failed after 3 attempts: Game installation appears corrupted"
            )
        }
    }
}