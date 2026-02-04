use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};
use tokio::fs;

use crate::game::patcher::get_version_manifest;

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

/// Ensures the game is installed and up to date (downloads JRE, Butler, and game)
pub async fn ensure_installed(
    client: &reqwest::Client,
    base_dir: &PathBuf,
    channel: &str,
    target_version: Option<i32>,
    policy: InstallPolicy,
    progress_callback: impl Fn(&str, f64, &str),
    cancel_token: Option<Arc<AtomicBool>>,
) -> Result<()> {
    progress_callback("check", 0.0, "Checking installation...");

    // --- FAST PATH: Offline Verification ---
    if policy == InstallPolicy::OfflineVerify {
        let ver_str = target_version
            .map(|v| v.to_string())
            .unwrap_or_else(|| "latest".to_string());
        let check_ver = if ver_str == "0" { "latest" } else { &ver_str };

        let paths = crate::game::paths::GamePaths::new(base_dir.clone());
        let game_ok = is_game_installed(base_dir, channel, check_ver).await;
        let jre_ok = paths.java_exec().exists();
        let butler_ok = paths.butler().exists();
        if game_ok && jre_ok && butler_ok {
            // Incluso en modo offline, intentamos verificar el agente (es muy rapido)
            if let Err(e) = crate::game::agent::ensure_agent(client, base_dir, &progress_callback, cancel_token.clone()).await {
                println!("[Install] Agent verification skipped or failed: {}", e);
            }
            progress_callback("complete", 100.0, "Verified.");
            return Ok(());
        }

        if !game_ok {
            progress_callback("check", 0.0, "Files missing, downloading...");
        } else if !jre_ok {
            progress_callback("check", 0.0, "JRE missing, downloading...");
        } else if !butler_ok {
            progress_callback("check", 0.0, "Butler missing, downloading...");
        }
    }
    // ----------------------------------------

    // --- NETWORK PATH: Full Update/Install ---

    // 1. Download JRE if needed
    crate::java::download_jre(client, base_dir, &progress_callback, cancel_token.clone()).await?;

    // 2. Install Butler if needed
    crate::game::patcher::install_butler(
        client,
        base_dir,
        &progress_callback,
        cancel_token.clone(),
    )
    .await?;

    // 2.5 Install DualAuth Agent
    crate::game::agent::ensure_agent(client, base_dir, &progress_callback, cancel_token.clone())
        .await?;

    // 3. Find latest version or use target
    progress_callback("version", 0.0, "Checking for game updates...");

    let requested_version = target_version.unwrap_or(0);
    let mut version_manifest =
        get_version_manifest(client, channel, base_dir, requested_version).await?;

    let user_version = version_manifest.user_version;
    let remote_version = version_manifest.latest_remote;
    let is_latest = user_version == 0;

    let install_dir_name = if is_latest {
        "latest".to_string()
    } else {
        user_version.to_string()
    };

    let target_ver_val = if is_latest {
        remote_version
    } else {
        user_version
    };

    let files_exist = is_game_installed(base_dir, channel, &install_dir_name).await;

    if files_exist && version_manifest.current_local == 0 && is_latest {
        progress_callback("check", 50.0, "Detected manual installation. Adopting...");

        // CLEANUP: Check if there's a leftover .original file from a previous patch/mod
        // and restore it to ensure we are adopting a clean state.
        let paths = crate::game::paths::GamePaths::new(base_dir.clone());
        let client_path = paths.client_exe(channel, &install_dir_name);

        let mut original_path = client_path.clone().into_os_string();
        original_path.push(".original");
        let original_path = PathBuf::from(original_path);

        if fs::metadata(&original_path).await.is_ok() {
            progress_callback(
                "check",
                55.0,
                "Found dirty patch. Restoring original binary...",
            );
            // Restore: move .original -> .exe (overwriting if necessary)
            fs::rename(&original_path, &client_path).await?;
            println!("Restored original binary from {:?}", original_path);
        }

        save_local_version(base_dir, channel, remote_version).await?;

        version_manifest.current_local = remote_version;
        println!("Manual installation adopted as version {}", remote_version);
    }

    // Verificar si ya esta al dia
    if files_exist && (!is_latest || version_manifest.current_local == remote_version) {
        progress_callback("complete", 100.0, "Game is up to date");
        return Ok(());
    }

    // 4. Download and install game
    let start_version = if is_latest && files_exist {
        version_manifest.current_local
    } else {
        0
    };

    if start_version == 0 {
        // --- FRESH INSTALL / DIRECT DOWNLOAD (Version 0 -> Target) ---
        progress_callback(
            "download",
            0.0,
            &format!("Installing game version {}...", target_ver_val),
        );

        let pwr_path = crate::game::patcher::download_pwr(
            client,
            channel,
            0,
            target_ver_val,
            &progress_callback,
            cancel_token.clone(),
        )
        .await?;

        // Apply patch (install)
        progress_callback("install", 0.0, "Installing game files...");
        crate::game::patcher::apply_pwr(
            base_dir,
            channel,
            &pwr_path,
            &install_dir_name,
            &progress_callback,
        )
        .await?;
    } else {
        // --- INCREMENTAL UPDATE (Step-by-Step) ---
        let mut steps: Vec<i32> = version_manifest
            .available_versions
            .iter()
            .cloned()
            .filter(|&v| v > start_version && v <= target_ver_val)
            .collect();

        steps.sort();

        if steps.is_empty() && start_version < target_ver_val {
            steps.push(target_ver_val);
        }

        let mut current_ver = start_version;
        let total_steps = steps.len();

        for (idx, next_ver) in steps.iter().enumerate() {
            progress_callback(
                "download",
                0.0,
                &format!(
                    "Updating part {}/{}: version {} -> {}...",
                    idx + 1,
                    total_steps,
                    current_ver,
                    next_ver
                ),
            );

            let pwr_path = crate::game::patcher::download_pwr(
                client,
                channel,
                current_ver,
                *next_ver,
                &progress_callback,
                cancel_token.clone(),
            )
            .await?;

            progress_callback(
                "install",
                0.0,
                &format!("Applying patch {}/{}...", idx + 1, total_steps),
            );

            crate::game::patcher::apply_pwr(
                base_dir,
                channel,
                &pwr_path,
                &install_dir_name,
                &progress_callback,
            )
            .await?;

            current_ver = *next_ver;

            if is_latest {
                let _ = save_local_version(base_dir, channel, current_ver).await;
            }
        }
    }

    // Verify critical file after patch
    let client_path =
        crate::game::paths::GamePaths::new(base_dir.clone()).client_exe(channel, &install_dir_name);

    if !fs::metadata(&client_path).await.is_ok() {
        anyhow::bail!(
            "Game installation incomplete: client executable not found after patching at {:?}",
            client_path
        );
    }

    let _ = crate::util::make_executable(&client_path).await;

    if is_latest {
        let _ = save_local_version(base_dir, channel, target_ver_val).await;
    }

    progress_callback("complete", 100.0, "Game installed successfully");

    if start_version != target_ver_val {
        let _ = crate::game::patcher::clean_patches_cache(&progress_callback).await;
    }

    Ok(())
}
