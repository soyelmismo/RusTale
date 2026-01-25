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

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub html_url: String,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

pub async fn check_for_updates(client: &Client) -> Result<Option<ReleaseInfo>> {
    let current_version = env!("CARGO_PKG_VERSION");
    if cfg!(debug_assertions) || current_version == "0.0.1" {
        println!(
            "[Updater] Development mode detected (v{}). Auto-update disabled.",
            current_version
        );
        return Ok(None);
    }

    let url = "https://api.github.com/repos/soyelmismo/RusTale/releases/latest";
    let response = client.get(url).send().await?;

    if !response.status().is_success() {
        return Ok(None);
    }

    let release: ReleaseInfo = response.json().await?;

    // Normalizar: quitar 'v' y espacios
    let remote_ver = release.tag_name.trim().trim_start_matches('v').to_string();
    let local_ver = current_version.trim().trim_start_matches('v').to_string();

    println!("[Updater] Local: v{}, Remote: v{}", local_ver, remote_ver);

    // Solo actualizar si son diferentes
    if remote_ver != local_ver {
        if get_asset_url(&release).is_some() {
            return Ok(Some(release));
        }
    }

    Ok(None)
}

pub fn get_asset_url(info: &ReleaseInfo) -> Option<String> {
    let target = if cfg!(windows) {
        "windows.zip"
    } else {
        "linux.zip"
    };

    info.assets
        .iter()
        .find(|a| a.name.to_lowercase().contains(target))
        .map(|a| a.browser_download_url.clone())
}

pub async fn perform_update(client: Client, asset_url: String) -> Result<()> {
    let current_exe = env::current_exe()?;
    let app_dir = current_exe.parent().context("No parent dir")?;

    let update_dir = app_dir.join("update_temp");
    if update_dir.exists() {
        tokio::fs::remove_dir_all(&update_dir).await?;
    }
    tokio::fs::create_dir_all(&update_dir).await?;

    let zip_path = update_dir.join("update.zip");

    println!("[Updater] Downloading ZIP from: {}", asset_url);

    let response = client.get(&asset_url).send().await?;
    if !response.status().is_success() {
        anyhow::bail!("Download failed: {}", response.status());
    }
    let content = response.bytes().await?;
    tokio::fs::write(&zip_path, content).await?;

    println!("[Updater] Extracting ZIP...");

    let update_dir_clone = update_dir.clone();
    let zip_path_clone = zip_path.clone();

    tokio::task::spawn_blocking(move || -> Result<()> {
        let file = std::fs::File::open(&zip_path_clone)?;
        let mut archive = zip::ZipArchive::new(file)?;
        archive.extract(&update_dir_clone)?;
        Ok(())
    })
    .await??;

    tokio::fs::remove_file(&zip_path).await?;

    println!("[Updater] Spawning update script...");
    spawn_update_script(&current_exe, &update_dir)?;

    Ok(())
}

fn spawn_update_script(current_exe: &Path, update_dir: &Path) -> Result<()> {
    let app_dir = current_exe.parent().unwrap();

    if cfg!(windows) {
        let script_content = format!(
            "@echo off\r\n\
             title RusTale Updating...\r\n\
             timeout /t 2 /nobreak >nul\r\n\
             echo Installing updates...\r\n\
             xcopy /s /y \"{}\\*\" \"{}\\\"\r\n\
             rmdir /s /q \"{}\"\r\n\
             start \"\" \"{}\"\r\n\
             del \"%~f0\"\r\n",
            update_dir.display(),
            app_dir.display(),
            update_dir.display(),
            current_exe.display()
        );

        let script_path = app_dir.join("updater.bat");
        std::fs::write(&script_path, script_content)?;

        std::process::Command::new("cmd")
            .args(["/C", &script_path.to_string_lossy()])
            .spawn()?;
    } else {
        let script_content = format!(
            "#!/bin/sh\n\
             sleep 2\n\
             cp -rf \"{}/.\" \"{}/\"\n\
             rm -rf \"{}\"\n\
             chmod +x \"{}\"\n\
             \"{}\" &\n",
            update_dir.display(),
            app_dir.display(),
            update_dir.display(),
            current_exe.display(),
            current_exe.display()
        );

        let script_path = app_dir.join("updater.sh");
        std::fs::write(&script_path, script_content)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script_path, perms)?;
        }

        std::process::Command::new("sh").arg(&script_path).spawn()?;
    }

    Ok(())
}
