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

    // Limpieza de versiones para comparación (v0.0.1 -> 0.0.1)
    let remote_ver_str = release.tag_name.trim_start_matches('v');
    let current_ver_str = current_version.trim_start_matches('v');

    println!(
        "[Updater] Local: {}, Remote: {}",
        current_ver_str, remote_ver_str
    );

    // Lógica simple: Si son diferentes y el remoto no es vacío, actualizamos.
    // Esto permite que builds tipo "nightly-build-123" disparen actualización sobre "0.0.1".
    if remote_ver_str != current_ver_str {
        // Verificar que existe un asset para nuestro sistema operativo antes de notificar
        if get_asset_url(&release).is_some() {
            return Ok(Some(release));
        }
    }

    Ok(None)
}

// Función auxiliar para obtener la URL correcta según el SO
pub fn get_asset_url(info: &ReleaseInfo) -> Option<String> {
    let target_substring = if cfg!(windows) {
        "windows" // Buscaremos 'rustale-windows.exe'
    } else {
        "linux" // Buscaremos 'rustale-linux'
    };

    info.assets
        .iter()
        .find(|a| a.name.to_lowercase().contains(target_substring))
        .map(|a| a.browser_download_url.clone())
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
        if let Ok(file) = std::fs::File::open(&temp_path) {
            let mut perms = file.metadata()?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&temp_path, perms)?;
        }
    }

    // 4. Spawn updater script/process
    spawn_update_process(&current_exe, &temp_path)?;

    Ok(())
}

fn spawn_update_process(current_exe: &Path, new_exe: &Path) -> Result<()> {
    let current_exe_name = current_exe.file_name().unwrap().to_string_lossy();
    let new_exe_name = new_exe.file_name().unwrap().to_string_lossy();

    if cfg!(windows) {
        // Windows: Script batch mejorado
        // El ping es un truco para esperar (timeout no siempre está en PATH)
        let script_content = format!(
            "@echo off\r\n\
             ping 127.0.0.1 -n 2 > nul\r\n\
             del /F \"{}\"\r\n\
             move /Y \"{}\" \"{}\"\r\n\
             start \"\" \"{}\"\r\n\
             (goto) 2>nul & del \"%~f0\"\r\n",
            current_exe_name, new_exe_name, current_exe_name, current_exe_name
        );

        let script_path = current_exe.parent().unwrap().join("update_launcher.bat");
        std::fs::write(&script_path, script_content)?;

        std::process::Command::new("cmd")
            .args(["/C", &script_path.to_string_lossy()])
            .spawn()?;
    } else {
        // Linux: shell script
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
