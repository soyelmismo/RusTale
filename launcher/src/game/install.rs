use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::atomic;
use tokio::fs;


/// Installation policy - defines the intent of the installation operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InstallPolicy {
    /// Only verify local file integrity. If files exist, skip network checks.
    /// Use this for the "PLAY" button to enable instant launches.
    OfflineVerify,
    /// Contact the server, verify manifests, and download if necessary.
    /// Use this for "UPDATE" or "INSTALL" operations.
    NetworkUpdate,
}

/// Checks if the game is installed for a specific version
pub async fn is_game_installed(base_dir: &PathBuf, channel: &str, version: &str) -> bool {
    let paths = crate::game::paths::GamePaths::new(base_dir.clone());
    let client_path = paths.client_exe(channel, version);
    fs::metadata(&client_path).await.is_ok()
}

/// Gets the local installed version (from latest folder metadata)
pub async fn get_local_version(base_dir: &PathBuf, channel: &str) -> Result<i32> {
    let version_file = base_dir.join(channel).join("version.json");

    if !fs::metadata(&version_file).await.is_ok() {
        return Ok(0);
    }

    let content = fs::read_to_string(&version_file)
        .await
        .context("Failed to read version file")?;
    let version_info: serde_json::Value = serde_json::from_str(&content)?;
    Ok(version_info
        .get("version")
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32)
}

pub async fn save_local_version(base_dir: &PathBuf, channel: &str, version: i32) -> Result<()> {
    let version_file = base_dir.join(channel).join("version.json");
    if let Some(parent) = version_file.parent() {
        fs::create_dir_all(parent).await?;
    }
    let version_info = serde_json::json!({ "version": version });
    fs::write(&version_file, serde_json::to_string_pretty(&version_info)?).await?;
    Ok(())
}

/// Helper to get all installed versions by scanning the directory
/// Returns a Vec of (version_number, is_latest_folder)
pub async fn get_installed_versions(base_dir: &PathBuf, channel: &str) -> Vec<(i32, bool)> {
    let channel_dir = base_dir.join(channel);
    let mut installed = Vec::new();

    // 1. Revisar carpetas numericas especificas (ej: "8", "9")
    if let Ok(mut entries) = fs::read_dir(&channel_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Ok(file_type) = entry.file_type().await {
                if file_type.is_dir() {
                    if let Some(name) = entry.file_name().to_str() {
                        if let Ok(ver) = name.parse::<i32>() {
                            // Validar que tenga el ejecutable
                            let client_path =
                                channel_dir
                                    .join(name)
                                    .join("Client")
                                    .join(if cfg!(windows) {
                                        "HytaleClient.exe"
                                    } else {
                                        "HytaleClient"
                                    });

                            if fs::metadata(&client_path).await.is_ok() {
                                installed.push((ver, false)); // false = carpeta explicita
                            }
                        }
                    }
                }
            }
        }
    }

    // 2. Revisar la carpeta 'latest'
    if let Ok(latest_ver) = get_local_version(base_dir, channel).await {
        if latest_ver > 0 {
            let latest_client_path =
                channel_dir
                    .join("latest")
                    .join("Client")
                    .join(if cfg!(windows) {
                        "HytaleClient.exe"
                    } else {
                        "HytaleClient"
                    });

            if fs::metadata(&latest_client_path).await.is_ok() {
                installed.push((latest_ver, true)); // true = carpeta latest
            }
        }
    }

    installed.sort_by(|a, b| b.0.cmp(&a.0));
    installed
}

/// Deletes a specific version
pub async fn delete_version(base_dir: &PathBuf, channel: &str, version: i32) -> Result<()> {
    let version_dir = base_dir.join(channel).join(version.to_string());

    if version_dir.exists() {
        fs::remove_dir_all(&version_dir).await?;
    }

    if let Ok(local_ver) = get_local_version(base_dir, channel).await {
        if local_ver == version {
            let latest_dir = base_dir.join(channel).join("latest");
            if latest_dir.exists() {
                fs::remove_dir_all(&latest_dir).await?;
            }
            let version_file = base_dir.join(channel).join("version.json");
            if version_file.exists() {
                let _ = fs::remove_file(version_file).await;
            }
        }
    }

    Ok(())
}
