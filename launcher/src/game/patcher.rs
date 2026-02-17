use anyhow::{Context, Result};
use futures::StreamExt;
use futures::io::AsyncBufReadExt as FuturesAsyncBufReadExt;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::io::BufReader;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufWriter};
use tokio::process::Command;

/// Game version information structure
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GameVersionInfo {
    pub user_version: i32,
    pub current_local: i32,
    pub latest_remote: i32,
    pub available_versions: Vec<i32>,
    pub available_versions_from_fallback: Option<Vec<i32>>,
    pub update_available: bool,
}

/// Applies a PWR patch file using butler
/// Enhanced with recovery logic and better error handling
pub async fn apply_pwr(
    root_dir: &PathBuf,
    channel: &str,
    install_dir_name: &str,
    pwr_path: &PathBuf,
    progress_callback: impl Fn(&str, f64, &str, u64, u64, Option<String>, Option<usize>),
    cancel_token: Option<Arc<AtomicBool>>,
) -> anyhow::Result<()> {
    let paths = crate::game::paths::GamePaths::new(root_dir.clone());
    let game_dir = paths.version_dir(channel, install_dir_name);
    let staging_dir = paths.staging();

    progress_callback("install", 0.0, "Applying patch...", 0, 0, None, None);

    // Verify directories exist before proceeding
    if !game_dir.exists() {
        anyhow::bail!(
            "Game directory does not exist after creation: {}",
            game_dir.display()
        );
    }
    if !staging_dir.exists() {
        anyhow::bail!(
            "Staging directory does not exist after creation: {}",
            staging_dir.display()
        );
    }

    println!(
        "[PATHS] Verified directories exist: game={}, staging={}",
        game_dir.display(),
        staging_dir.display()
    );

    // Final verification right before Butler command
    // This prevents the "OutputFolder must exist" error from Butler
    for attempt in 0..5 {
        if game_dir.exists() && staging_dir.exists() {
            println!(
                "[PATHS] Final directory verification passed on attempt {}",
                attempt + 1
            );
            break;
        } else if attempt < 4 {
            println!(
                "[PATHS] Directory verification failed, retrying in 200ms... (attempt {})",
                attempt + 1
            );
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        } else {
            anyhow::bail!(
                "Critical: Directories do not exist immediately before Butler start: game={}, staging={}",
                game_dir.exists(),
                staging_dir.exists()
            );
        }
    }

    // Use Butler to apply the patch
    let butler_path = paths.butler();
    let pwr_path_absolute =
        std::fs::canonicalize(pwr_path).context("Failed to canonicalize PWR path")?;

    // Validate patch file integrity before attempting to apply
    progress_callback("install", 2.0, "Validating patch file...", 0, 0, None, None);

    // Create IntegrityChecker instance
    let integrity_checker = crate::game::patch_api::IntegrityChecker::new();

    integrity_checker
        .validate_patch_file(&pwr_path)
        .await
        .context("Patch file validation failed - file may be corrupted")?;

    // Enhanced retry logic for patch application
    let mut last_error = None;

    for attempt in 1..=2 {
        if attempt > 1 {
            println!("[RETRY] Retrying patch application (attempt {})", attempt);
            progress_callback(
                "install",
                5.0,
                &format!("Retrying patch application (attempt {})...", attempt),
                0,
                0,
                None,
                None,
            );

            // Clean up game directory for retry
            if game_dir.exists() {
                println!("[CLEANUP] Removing corrupted game directory for retry");
                let _ = tokio::fs::remove_dir_all(&game_dir).await;
                // Recreate directories using the path methods (they auto-create)
                let _ = paths.version_dir(channel, install_dir_name);
            }
            if staging_dir.exists() {
                let _ = tokio::fs::remove_dir_all(&staging_dir).await;
                let _ = paths.staging();
            }
        } else {
            progress_callback(
                "install",
                5.0,
                "Preparing patch application...",
                0,
                0,
                None,
                None,
            );
        }

        let mut cmd = Command::new(&butler_path);
        cmd.arg("apply")
            .arg(format!("--staging-dir={}", staging_dir.display()))
            .arg(&pwr_path_absolute)
            .arg(&game_dir);

        progress_callback("install", 10.0, "Extracting patch...", 0, 0, None, None);

        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to start Butler")?;

        // Create log file for Butler output
        let logs_dir = paths.logs();
        let log_path = logs_dir.join(format!(
            "butler_apply_{}_attempt_{}.log",
            chrono::Utc::now().timestamp(),
            attempt
        ));
        let log_file: tokio::fs::File = tokio::fs::File::create(&log_path)
            .await
            .context("Failed to create Butler log file")?;
        let mut log_writer = BufWriter::new(log_file);

        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        // Spawn a task to handle stderr
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut stderr_reader = BufReader::new(stderr);
            let mut line = String::new();
            loop {
                line.clear();
                match stderr_reader.read_line(&mut line).await {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        eprintln!("[Butler Error] {}", line.trim());
                    }
                    Err(e) => {
                        eprintln!("[Butler Error reading stderr] {}", e);
                        break;
                    }
                }
            }
        });

        let mut current_pct = 0.0;
        let mut line_buf = Vec::new();
        let mut stdout_reader = BufReader::new(stdout);

        // Use read_until(b'\r') because Butler uses \r to update progress without newlines
        while let Ok(n) = stdout_reader.read_until(b'\r', &mut line_buf).await {
            // Check for cancellation
            if let Some(token) = &cancel_token {
                if token.load(Ordering::Relaxed) {
                    let _ = child.kill().await;
                    return Err(anyhow::anyhow!("Operation cancelled"));
                }
            }

            if n == 0 {
                break;
            }

            let raw_s = String::from_utf8_lossy(&line_buf);
            // Split by \n just in case Butler mixes them
            for line in raw_s.split('\n') {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                // Log to file
                let _ = log_writer.write_all(format!("{}\n", line).as_bytes()).await;
                let _ = log_writer.flush().await;

                if line.contains('%') {
                    if let Some(pct) = parse_butler_line(line) {
                        current_pct = pct as f64;
                        let file = line.split('%').last().unwrap_or("").trim();
                        let status = if attempt > 1 {
                            format!("Retry {} - Extracting: {}", attempt, file)
                        } else {
                            file.to_string()
                        };
                        progress_callback("install", current_pct, &status, 0, 0, None, None);
                    }
                } else {
                    if line.len() < 100 && !line.starts_with("\u{2590}") {
                        let status = if attempt > 1 {
                            format!("Retry {} - {}", attempt, line)
                        } else {
                            line.to_string()
                        };
                        progress_callback("install", current_pct, &status, 0, 0, None, None);
                    }
                }
            }
            line_buf.clear();
        }

        let status = child.wait().await?;
        if status.success() {
            // SUCCESS: Verify extraction integrity before reporting success
            progress_callback(
                "install",
                95.0,
                "Verifying installation...",
                0,
                0,
                None,
                None,
            );

            match integrity_checker
                .verify_extraction_integrity(&game_dir)
                .await
            {
                Ok(_) => {
                    progress_callback(
                        "install",
                        100.0,
                        "Patch applied successfully",
                        0,
                        0,
                        None,
                        None,
                    );
                    println!(
                        "[SUCCESS] Patch application and verification completed on attempt {}",
                        attempt
                    );
                    return Ok(());
                }
                Err(e) => {
                    last_error = Some(anyhow::anyhow!(
                        "Extraction verification failed on attempt {}: {}",
                        attempt,
                        e
                    ));
                    println!(
                        "[ERROR] Extraction verification failed on attempt {}: {}",
                        attempt, e
                    );

                    // Log the verification failure
                    let _ = log_writer
                        .write_all(
                            format!("\n[ERROR] Extraction verification failed: {}\n", e).as_bytes(),
                        )
                        .await;
                    let _ = log_writer.flush().await;

                    // Continue to retry
                }
            }
        } else {
            // Butler failed
            let stderr_output: String = tokio::fs::read_to_string(&log_path)
                .await
                .unwrap_or_default();

            if stderr_output.contains("already up to date") {
                println!("Game is already up to date");
                return Ok(());
            }

            last_error = Some(anyhow::anyhow!(
                "Butler patch application failed on attempt {}: {}",
                attempt,
                stderr_output
            ));
            println!(
                "[ERROR] Butler patch application failed on attempt {}: {}",
                attempt, stderr_output
            );
        }

        // Clean up for retry
        if game_dir.exists() {
            let _ = tokio::fs::remove_dir_all(&game_dir).await;
        }
    }

    // All attempts failed
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Patch application failed after 2 attempts")))
}

/// Helper function to parse Butler progress line
fn parse_butler_line(line: &str) -> Option<f32> {
    // Butler outputs progress like: "Progress: 45.23% (1234/5678)"
    if let Some(progress_start) = line.find("Progress: ") {
        let progress_part = &line[progress_start + 11..];
        if let Some(percent_end) = progress_part.find('%') {
            let percent_str = &progress_part[..percent_end];
            return percent_str.parse().ok();
        }
    }
    None
}

/// Cleans up the patches cache directory using the shared cache system
pub async fn clean_patches_cache(
    progress_callback: impl Fn(f32, &str, u64, u64, Option<String>, Option<usize>),
) -> Result<()> {
    let base_dir = crate::config::get_app_dir();

    progress_callback(0.0, "Cleaning patches cache...", 0, 0, None, None);

    // Use the shared cache manager for cleanup
    let cleaned = crate::game::patch_api::get_shared_cache()
        .cleanup_old_patches()
        .await?;

    progress_callback(
        100.0,
        &format!("Cleaned {} cache files", cleaned),
        0,
        0,
        None,
        None,
    );
    Ok(())
}
