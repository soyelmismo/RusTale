use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::io::BufReader;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufWriter};
use tokio::process::Command;
use crate::progress::ProgressCallback;
use crate::paths::GamePaths;

/// Applies a PWR patch file using butler
/// Enhanced with recovery logic and better error handling
pub async fn apply_pwr(
    root_dir: &PathBuf,
    channel: &str,
    install_dir_name: &str,
    pwr_path: &PathBuf,
    progress_callback: ProgressCallback,
    cancel_token: Option<Arc<AtomicBool>>,
    localization: &crate::lang::Localization,
) -> Result<()> {
    let paths = GamePaths::new(root_dir.clone());
    let game_dir = paths.version_dir(channel, install_dir_name);
    let staging_dir = paths.staging();

    progress_callback("install".to_string(), 0.0, localization.t("common.applying_patch").to_string(), 0, 0, None, None);

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

    // Final verification right before Butler command
    for attempt in 0..5 {
        if !game_dir.exists() {
            let _ = std::fs::create_dir_all(&game_dir);
        }
        if !staging_dir.exists() {
            let _ = std::fs::create_dir_all(&staging_dir);
        }

        if game_dir.exists() && staging_dir.exists() {
            break;
        } else if attempt < 4 {
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        } else {
            anyhow::bail!(
                "Critical: Directories do not exist immediately before Butler start: game={}, staging={}",
                game_dir.exists(),
                staging_dir.exists()
            );
        }
    }

    let butler_path = paths.butler();
    let pwr_path_absolute =
        std::fs::canonicalize(pwr_path).context("Failed to canonicalize PWR path")?;

    progress_callback("install".to_string(), 2.0, localization.t("common.validating_patch").to_string(), 0, 0, None, Some(1));

    let integrity_checker = crate::patch_api::integrity_checker::IntegrityChecker::new();

    integrity_checker
        .validate_patch_file(&pwr_path)
        .await
        .context("Patch file validation failed - file may be corrupted")?;

    let mut last_error = None;

    for attempt in 1..=2 {
        if staging_dir.exists() {
            let _ = tokio::fs::remove_dir_all(&staging_dir).await;
        }
        let _ = paths.staging();

        if attempt > 1 {
            progress_callback(
                "install".to_string(),
                5.0,
                format!("Retrying patch application (attempt {})...", attempt),
                0,
                0,
                None,
                Some(2),
            );
        } else {
            progress_callback(
                "install".to_string(),
                5.0,
                "Preparing patch application...".to_string(),
                0,
                0,
                None,
                Some(2),
            );
        }

        if !game_dir.exists() {
             let _ = std::fs::create_dir_all(&game_dir);
        }

        let mut cmd = Command::new(&butler_path);
        cmd.arg("apply")
            .arg(format!("--staging-dir={}", staging_dir.display()))
            .arg(&pwr_path_absolute)
            .arg(&game_dir);

        #[cfg(windows)]
        {
            cmd.creation_flags(0x08000000);
        }

        progress_callback("install".to_string(), 10.0, "Extracting patch...".to_string(), 0, 0, None, Some(3));

        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to start Butler")?;

        let logs_dir = paths.logs();
        let log_path = logs_dir.join(format!(
            "butler_apply_{}_attempt_{}.log",
            chrono::Utc::now().timestamp(),
            attempt
        ));
        let log_file = tokio::fs::File::create(&log_path)
            .await
            .context("Failed to create Butler log file")?;
        let mut log_writer = BufWriter::new(log_file);

        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        let log_path_stderr = log_path.clone();
        
        tokio::spawn(async move {
            let mut stderr_reader = BufReader::new(stderr);
            let mut line = String::new();
            
            let mut log_file = tokio::fs::OpenOptions::new()
                .append(true)
                .open(&log_path_stderr)
                .await;

            loop {
                line.clear();
                match stderr_reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => {
                        let trimmed = line.trim();
                        if let Ok(ref mut file) = log_file {
                            let _ = file.write_all(format!("[STDERR] {}\n", trimmed).as_bytes()).await;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let mut current_pct = 0.0;
        let mut line_buf = Vec::new();
        let mut stdout_reader = BufReader::new(stdout);

        while let Ok(n) = stdout_reader.read_until(b'\r', &mut line_buf).await {
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
            for line in raw_s.split('\n') {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                let _ = log_writer.write_all(format!("{}\n", line).as_bytes()).await;
                let _ = log_writer.flush().await;

                if line.contains('%') {
                    if let Some(pct) = parse_butler_line(line) {
                        current_pct = pct as f64;
                        let file = line.split('%').last().unwrap_or("").trim();
                        
                        let status = if attempt > 1 {
                            format!("Retry - {}", file)
                        } else {
                            file.to_string()
                        };
                        progress_callback("install".to_string(), current_pct, status, 0, 0, None, Some(3));
                    }
                } else {
                    if line.len() < 100 && !line.starts_with("\u{2590}") {
                        let status = if attempt > 1 {
                            format!("Retry {} - {}", attempt, line)
                        } else {
                            line.to_string()
                        };
                        progress_callback("install".to_string(), current_pct, status, 0, 0, None, Some(3));
                    }
                }
            }
            line_buf.clear();
        }

        let stderr_output = tokio::fs::read_to_string(&log_path)
            .await
            .unwrap_or_default();

        let status: std::process::ExitStatus = child.wait().await?;
        if status.success() {
            progress_callback(
                "install".to_string(),
                95.0,
                "Verifying installation...".to_string(),
                0,
                0,
                None,
                Some(4),
            );

            match integrity_checker
                .verify_extraction_integrity(&game_dir)
                .await
            {
                Ok(_) => {
                    progress_callback(
                        "install".to_string(),
                        100.0,
                        localization.t("common.patch_applied_successfully").to_string(),
                        0,
                        0,
                        None,
                        Some(4),
                    );
                    return Ok(());
                }
                Err(e) => {
                    last_error = Some(anyhow::anyhow!(
                        "Extraction verification failed on attempt {}: {}",
                        attempt,
                        e
                    ));
                }
            }
        } else {
            if stderr_output.contains("already up to date") {
                return Ok(());
            }

            last_error = Some(anyhow::anyhow!(
                "Butler patch application failed on attempt {}: {}",
                attempt,
                stderr_output
            ));
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Patch application failed after 2 attempts")))
}

/// Helper function to parse Butler progress line
fn parse_butler_line(line: &str) -> Option<f32> {
    if let Some(progress_start) = line.find("Progress: ") {
        let progress_part = &line[progress_start + 11..];
        if let Some(percent_end) = progress_part.find('%') {
            let percent_str = &progress_part[..percent_end];
            return percent_str.parse().ok();
        }
    }
    None
}
