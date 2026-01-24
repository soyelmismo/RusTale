use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use std::env;
use std::path::Path;

#[derive(Debug, Clone)]
pub enum UpdaterMessage {
    CheckForUpdates,
    UpdateFound(ReleaseInfo),
    UpdateNotFound,
    Error(String),
    StartUpdate(String),
    UpdateProgress(f32, String),
    UpdateFinished,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub html_url: String,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

pub async fn check_for_updates(client: &Client) -> Result<Option<ReleaseInfo>> {
    let url = "https://api.github.com/repos/soyelmismo/RusTale/releases/latest";
    let response = client.get(url).send().await?;

    if !response.status().is_success() {
        return Ok(None);
    }

    let release: ReleaseInfo = response.json().await?;
    let current_version = env!("CARGO_PKG_VERSION");

    // Remove 'v' prefix if present for comparison
    let remote_ver_str = release.tag_name.trim_start_matches('v');
    let current_ver_str = current_version.trim_start_matches('v');

    if compare_versions(remote_ver_str, current_ver_str) {
        Ok(Some(release))
    } else {
        Ok(None)
    }
}

// Returns true if remote > current
fn compare_versions(remote: &str, current: &str) -> bool {
    let r_parts: Vec<&str> = remote.split('.').collect();
    let c_parts: Vec<&str> = current.split('.').collect();

    for i in 0..std::cmp::max(r_parts.len(), c_parts.len()) {
        let r_val = r_parts.get(i).unwrap_or(&"0").parse::<u32>().unwrap_or(0);
        let c_val = c_parts.get(i).unwrap_or(&"0").parse::<u32>().unwrap_or(0);

        if r_val > c_val {
            return true;
        }
        if r_val < c_val {
            return false;
        }
    }
    false
}

pub async fn perform_update(client: Client, asset_url: String) -> Result<()> {
    // 1. Determine temporary file path
    let current_exe = env::current_exe()?;
    let current_dir = current_exe.parent().context("No parent dir")?;

    let temp_name = if cfg!(windows) {
        "rustale_new.exe"
    } else {
        "rustale_new"
    };
    let temp_path = current_dir.join(temp_name);

    // 2. Download
    let response = client.get(&asset_url).send().await?;
    if !response.status().is_success() {
        return Err(anyhow::anyhow!("Failed to download update"));
    }

    let content = response.bytes().await?;
    tokio::fs::write(&temp_path, content).await?;

    // 3. Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&temp_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&temp_path, perms)?;
    }

    // 4. Spawn updater script/process
    spawn_update_process(&current_exe, &temp_path)?;

    Ok(())
}

fn spawn_update_process(current_exe: &Path, new_exe: &Path) -> Result<()> {
    let current_exe_name = current_exe.file_name().unwrap().to_string_lossy();
    let new_exe_name = new_exe.file_name().unwrap().to_string_lossy();

    if cfg!(windows) {
        // Windows: Create a batch script to sleep, move, and restart
        let script_content = format!(
            "@echo off\r\n\
             timeout /t 2 /nobreak >nul\r\n\
             move /y \"{}\" \"{}\"\r\n\
             start \"\" \"{}\"\r\n\
             del \"%~f0\"\r\n",
            new_exe_name, current_exe_name, current_exe_name
        );

        let script_path = current_exe.parent().unwrap().join("update.bat");
        std::fs::write(&script_path, script_content)?;

        std::process::Command::new("cmd")
            .args(["/C", &script_path.to_string_lossy()])
            .spawn()?;
    } else {
        // Linux: simpler, just rename over (Linux allows replacing running open files)
        // However, since we want to restart, we might as well just exec?
        // No, let's do a simple shell spawn
        let script_content = format!(
            "sleep 1; mv -f \"{}\" \"{}\"; \"{}\" &",
            new_exe_name,
            current_exe_name,
            current_exe.to_string_lossy()
        );

        std::process::Command::new("sh")
            .arg("-c")
            .arg(script_content)
            .spawn()?;
    }

    Ok(())
}
