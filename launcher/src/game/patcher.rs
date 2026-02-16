use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};
use tokio::process::Command;
use tokio::io::{AsyncWriteExt, AsyncBufReadExt, BufWriter};
use tokio::io::BufReader;
use std::process::Stdio;
use futures::io::{AsyncBufReadExt as FuturesAsyncBufReadExt};
use futures::StreamExt;

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
pub async fn apply_pwr(
    root_dir: &PathBuf,
    channel: &str,
    install_dir_name: &str,
    pwr_path: &PathBuf,
    progress_callback: impl Fn(&str, f64, &str, u64, u64, Option<String>, Option<usize>),
    cancel_token: Option<Arc<AtomicBool>>,
) -> anyhow::Result<()> {
    let paths = crate::game::paths::GamePaths::new(root_dir.clone());
    let game_dir = paths.game_dir().join(install_dir_name);

    // Ensure game directory exists
    tokio::fs::create_dir_all(&game_dir).await
        .context("Failed to create game directory")?;

    progress_callback("install", 0.0, "Applying patch...", 0, 0, None, None);

    // Use Butler to apply the patch
    let butler_path = paths.butler();
    let pwr_path_absolute = std::fs::canonicalize(pwr_path)
        .context("Failed to canonicalize PWR path")?;

    let mut cmd = Command::new(&butler_path);
    cmd.arg("apply")
        .arg(&pwr_path_absolute)
        .arg(&game_dir);

    progress_callback("install", 10.0, "Extracting patch...", 0, 0, None, None);

    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to start Butler")?;

    // Create log file for Butler output
    let log_path = paths.logs().join(format!("butler_apply_{}.log", chrono::Utc::now().timestamp()));
    let log_file: tokio::fs::File = tokio::fs::File::create(&log_path).await
        .context("Failed to create Butler log file")?;
    let mut log_writer = BufWriter::new(log_file);

    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();

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
                    progress_callback("install", current_pct, file, 0, 0, None, None);
                }
            } else {
                if line.len() < 100 && !line.starts_with("\u{2590}") {
                    progress_callback("install", current_pct, line, 0, 0, None, None);
                }
            }
        }
        line_buf.clear();
    }

    let status = child.wait().await?;
    if !status.success() {
        // --- NEW: Recovery logic ---
        let stderr_output: String = tokio::fs::read_to_string(&log_path).await.unwrap_or_default();
        
        if stderr_output.contains("already up to date") {
            println!("Game is already up to date");
            return Ok(());
        }
        
        anyhow::bail!("Butler patch application failed: {}", stderr_output);
    }

    progress_callback("install", 100.0, "Patch applied successfully", 0, 0, None, None);
    Ok(())
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

/// Downloads a file with automatic fallback using the new patch API system
pub async fn download_with_fallback<F>(
    client: &reqwest::Client,
    primary_url: &str,
    dest_path: &PathBuf,
    progress_callback: impl Fn(f32, &str, u64, u64, Option<String>),
    cancel_token: Option<Arc<AtomicBool>>,
    fallback_url_resolver: F,
    file_type: &str,
) -> anyhow::Result<()>
where
    F: FnOnce() -> anyhow::Result<String>,
{
    // Try primary download first
    let download_result = crate::game::downloader::download_file(
        client,
        primary_url,
        dest_path,
        |pct, speed, total, downloaded, eta| {
            progress_callback(pct, &speed, total, downloaded, eta);
        },
        cancel_token.clone(),
    ).await;

    // If primary fails, try fallback
    if download_result.is_err() {
        println!("Main {} download failed, trying fallback API...", file_type);
        match fallback_url_resolver() {
            Ok(fallback_url) => {
                println!("Using fallback URL: {}", fallback_url);
                crate::game::downloader::download_file(
                    client,
                    &fallback_url,
                    dest_path,
                    |pct, speed, total, downloaded, eta| {
                        progress_callback(pct, &format!("Fallback download... ({})", speed), total, downloaded, eta);
                    },
                    cancel_token,
                ).await?;
            }
            Err(e) => {
                anyhow::bail!("Failed to get fallback URL: {}", e);
            }
        }
    }

    Ok(())
}

/// Cleans up the patches cache directory using the shared cache system
pub async fn clean_patches_cache(
    progress_callback: impl Fn(f32, &str, u64, u64, Option<String>, Option<usize>),
) -> Result<()> {
    let base_dir = crate::config::get_app_dir();
    
    progress_callback(0.0, "Cleaning patches cache...", 0, 0, None, None);

    // Use the shared cache manager for cleanup
    let cleaned = crate::game::patch_api::get_shared_cache()
        .cleanup_old_patches(&base_dir).await?;

    progress_callback(100.0, &format!("Cleaned {} cache files", cleaned), 0, 0, None, None);
    Ok(())
}

/// Helper function to get architecture name
pub fn get_arch_name() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    }
}
