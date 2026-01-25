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
        "windows.exe"
    } else {
        "linux"
    };

    info.assets
        .iter()
        .find(|a| a.name.to_lowercase().contains(target))
        .map(|a| a.browser_download_url.clone())
}

pub async fn perform_update(client: Client, asset_url: String) -> Result<()> {
    let current_exe = env::current_exe()?;
    let current_dir = current_exe.parent().context("No parent dir")?;

    let temp_name = if cfg!(windows) {
        "rustale_new.exe"
    } else {
        "rustale_new"
    };
    let temp_path = current_dir.join(temp_name);

    println!("[Updater] Downloading from: {}", asset_url);

    let response = client.get(&asset_url).send().await?;
    if !response.status().is_success() {
        anyhow::bail!("Download failed: {}", response.status());
    }

    let content = response.bytes().await?;
    tokio::fs::write(&temp_path, content).await?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let file = std::fs::File::open(&temp_path)?;
        let mut perms = file.metadata()?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&temp_path, perms)?;
    }

    spawn_update_process(&current_exe, &temp_path)?;
    Ok(())
}

fn spawn_update_process(current: &Path, new: &Path) -> Result<()> {
    let current_abs = std::fs::canonicalize(current)?;
    let new_abs = std::fs::canonicalize(new)?;
    let current_dir = current_abs.parent().unwrap();

    if cfg!(windows) {
        let script = format!(
            "@echo off\r\n\
             timeout /t 1 /nobreak >nul\r\n\
             del /F \"{}\"\r\n\
             move /Y \"{}\" \"{}\"\r\n\
             start \"\" \"{}\"\r\n\
             del \"%~f0\"\r\n",
            current_abs.display(),
            new_abs.display(),
            current_abs.display(),
            current_abs.display()
        );
        let script_path = current_dir.join("update.bat");
        std::fs::write(&script_path, script)?;

        std::process::Command::new("cmd")
            .args(["/C", &script_path.to_string_lossy()])
            .spawn()?;
    } else {
        // En Linux usamos rutas absolutas para el mv y el reinicio
        let script = format!(
            "sleep 1; chmod +x \"{}\"; mv -f \"{}\" \"{}\"; \"{}\" &",
            new_abs.display(),
            new_abs.display(),
            current_abs.display(),
            current_abs.display()
        );
        std::process::Command::new("sh")
            .arg("-c")
            .arg(script)
            .spawn()?;
    }
    Ok(())
}
