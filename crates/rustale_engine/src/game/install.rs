use std::path::PathBuf;
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
    /// Force full hash verification and download missing/corrupted files.
    Repair,
}

/// Checks if the game is installed for a specific version
pub async fn is_game_installed(base_dir: &PathBuf, channel: &str, version: &str) -> bool {
    let paths = crate::game::paths::GamePaths::new(base_dir.clone());
    let client_path = paths.client_exe(channel, version);
    fs::metadata(&client_path).await.is_ok()
}

/// Gets the local installed version (from latest folder metadata)
pub use crate::game::patch_api::{get_local_version, save_local_version};

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
