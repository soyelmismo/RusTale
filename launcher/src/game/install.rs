use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};
use tokio::fs;

use crate::game::patch_api::compat::{get_version_manifest, download_pwr};
use crate::game::patch_api::get_arch_name;

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
    progress_callback: impl Fn(&str, f64, &str, u64, u64, Option<String>, Option<usize>),
    cancel_token: Option<Arc<AtomicBool>>,
) -> Result<(usize, ())> {
    progress_callback("check", 0.0, "Checking installation...", 0, 0, None, None);

    // --- FAST PATH: Offline Verification ---
    if policy == InstallPolicy::OfflineVerify {
        let paths = crate::game::paths::GamePaths::new(base_dir.clone());
        
        // For offline verification, we need to check if all components are available locally
        let ver_str = target_version
            .map(|v| v.to_string())
            .unwrap_or_else(|| "latest".to_string());
        let check_ver = if ver_str == "0" { "latest" } else { &ver_str };
        
        // Check if game files exist
        let game_ok = is_game_installed(base_dir, channel, check_ver).await;
        
        // Check if JRE is available
        let tools_dir = base_dir.join("tools").join("jre");
        let jre_ok = crate::java::is_jre_installed_at(&tools_dir.join("latest"));
        
        // Check if Butler is available
        let butler_ok = paths.butler().exists();
        
        // If everything is OK, we can run offline
        if game_ok && jre_ok && butler_ok {
            progress_callback("complete", 100.0, "Verified.", 0, 0, None, None);
            return Ok((0, ()));
        }

        // If something is missing, fall through to network update
        if !game_ok {
            progress_callback("check", 0.0, "Files missing, downloading...", 0, 0, None, None);
        } else if !jre_ok {
            progress_callback("check", 0.0, "JRE missing, downloading...", 0, 0, None, None);
        } else if !butler_ok {
            progress_callback("check", 0.0, "Butler missing, downloading...", 0, 0, None, None);
        }
    }
    // ----------------------------------------

    // --- NETWORK PATH: Full Update/Install ---

    // 1. Ensure JRE is available (only downloads if needed)
    println!("[JRE Debug] game/install.rs - base_dir: {}", base_dir.display());
    let _java_info = crate::java_detection::ensure_java_available(base_dir).await?;

    // 2. Install Butler if needed
    crate::game::patch_api::compat::install_butler(
        client,
        base_dir,
        &progress_callback,
        cancel_token.clone(),
    )
    .await?;

    // 2.5 Skip DualAuth Agent download during installation - will be downloaded async after game launch
    println!("[Install] Skipping agent download during installation - will download after game launch");

    // 3. Find latest version or use target
    progress_callback("version", 0.0, "Checking for game updates...", 0, 0, None, None);

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

    // 4. Download and install game
    let start_version = if is_latest && files_exist {
        version_manifest.current_local
    } else {
        0
    };

    if files_exist && version_manifest.current_local == 0 && is_latest {
        progress_callback("check", 50.0, "Detected manual installation. Adopting...", 0, 0, None, None);

        save_local_version(base_dir, channel, remote_version).await?;

        version_manifest.current_local = remote_version;
        println!("Manual installation adopted as version {}", remote_version);
    }

    // Verificar si ya esta al dia
    if files_exist && (!is_latest || version_manifest.current_local == remote_version) {
        progress_callback("complete", 100.0, "Game is up to date", 0, 0, None, None);
        return Ok((0, ()));
    }

    // --- OPTIMIZED DOWNLOAD/INSTALL STRATEGY ---
    // Always find the highest complete version ≤ target for optimal downloads
    let mut highest_complete_version = 0;
    
    // Check for complete versions using PatchApiManager
    let manager = crate::game::patch_api::PatchApiManager::new();
    
    // Get available versions first
    let available_versions = if let Some(ref fallback_versions) = version_manifest.available_versions_from_fallback {
        fallback_versions.clone()
    } else {
        version_manifest.available_versions.clone()
    };
    
    // Find highest complete version ≤ target
    for &version in available_versions.iter().rev() { // Check from highest to lowest
        if version <= target_ver_val && version > 0 {
            if manager.has_complete_version(channel, std::env::consts::OS, get_arch_name(), version).await {
                highest_complete_version = version;
                println!("Found complete version {} for target {}", version, target_ver_val);
                break;
            }
        }
    }
    
    let base_start_version = if highest_complete_version > 0 {
        0  // Always start from 0 when we have a complete version
    } else {
        start_version
    };
    
    println!("Using base version: {} (highest_complete: {}, start: {})", base_start_version, highest_complete_version, start_version);

    // --- PHASE 1: DOWNLOAD ALL PATCHES ---
    let mut patch_files = Vec::new();
    let mut current_download_start = base_start_version;
    let has_complete_version = highest_complete_version > 0;
    
    // If we have a complete version > 0, download it first
    if highest_complete_version > 0 {
        progress_callback(
            "download",
            0.0,
            &format!("Downloading complete version {} (0 -> {})", highest_complete_version, highest_complete_version),
            0,
            0,
            None,
            Some(1), // Step 1: complete version
        );
        
        match download_pwr(
            client,
            channel,
            0,
            highest_complete_version,
            &progress_callback,
            cancel_token.clone(),
        ).await {
            Ok(pwr_path) => {
                patch_files.push((pwr_path, highest_complete_version));
                current_download_start = highest_complete_version;
            }
            Err(e) => {
                println!("Failed to download complete version {}: {}", highest_complete_version, e);
                // Fall back to incremental from 0
                current_download_start = 0;
            }
        }
    }
    
    // Download incremental patches from current_start to target
    if current_download_start < target_ver_val {
        // Get available versions for incremental patches
        let available_versions = if let Some(ref fallback_versions) = version_manifest.available_versions_from_fallback {
            println!("Using fallback versions for incremental update: {:?}", fallback_versions);
            fallback_versions.clone()
        } else {
            println!("Using default versions for incremental update: {:?}", version_manifest.available_versions);
            version_manifest.available_versions.clone()
        };
        
        // Filter versions that are greater than current_download_start and less than or equal to target
        let mut steps: Vec<i32> = available_versions
            .iter()
            .cloned()
            .filter(|&v| v > current_download_start && v <= target_ver_val)
            .collect();
        
        steps.sort();
        
        // If no steps found but we need to update, create direct step from start to target
        if steps.is_empty() && current_download_start < target_ver_val {
            println!("No intermediate versions found, will download directly from {} to {}", current_download_start, target_ver_val);
            steps.push(target_ver_val);
        }
        
        println!("Incremental patches to download: {:?}", steps);
        
        // Calculate total steps early for UI progress tracking
        let total_download_steps = if has_complete_version {
            // If we have complete version + incrementals
            (1 + steps.len()) as u64 // 1 for complete + number of incrementals
        } else {
            // If only incrementals from 0
            steps.len() as u64
        };
        
        // Update the complete version download callback with proper steps
        if has_complete_version {
            progress_callback(
                "download",
                0.0,
                &format!("Downloading patch 1/{}: 0 -> {}", total_download_steps, highest_complete_version),
                1,
                total_download_steps,
                None,
                Some(1), // Step 1: complete version
            );
        }
        
        let mut step_index = 0;
        while step_index < steps.len() && current_download_start < target_ver_val {
            let next_ver = steps[step_index];
            
            // Skip versions that are already installed
            if next_ver <= current_download_start {
                step_index += 1;
                continue;
            }
            
            let current_step = if has_complete_version {
                (step_index + 2) as u64 // +2 because: 1 for complete version + step_index+1 for current
            } else {
                (step_index + 1) as u64 // step_index+1 because it's 0-based
            };
            
            progress_callback(
                "download",
                ((step_index as f32 / steps.len() as f32) * 100.0) as f64,
                &format!(
                    "Downloading patch {}/{}: {} -> {}",
                    current_step,
                    total_download_steps,
                    current_download_start,
                    next_ver
                ),
                current_step as u64,
                total_download_steps,
                None,
                Some(current_step as usize), // Pasar el current_step calculado
            );
            
            // Crear un wrapper que capture el current_step para pasarlo a download_pwr
            let current_step_for_download = current_step;
            let progress_callback_wrapper = |phase: &str, sub_p: f64, msg: &str, total_bytes: u64, downloaded_bytes: u64, eta: Option<String>, _current_step: Option<usize>| {
                progress_callback(phase, sub_p, msg, total_bytes, downloaded_bytes, eta, Some(current_step_for_download as usize));
            };
            
            match download_pwr(
                client,
                channel,
                current_download_start,
                next_ver,
                &progress_callback_wrapper,
                cancel_token.clone(),
            ).await {
                Ok(pwr_path) => {
                    patch_files.push((pwr_path, next_ver));
                    current_download_start = next_ver;
                    step_index += 1; // Move to next step
                }
                Err(e) => {
                    println!("Failed to download patch {}->{}: {}", current_download_start, next_ver, e);
                    
                    // Look for the largest available patch from current_download_start
                    let mut found_larger_patch = false;
                    let mut next_step_index = step_index + 1;
                    
                    while next_step_index < steps.len() {
                        let future_ver = steps[next_step_index];
                        match download_pwr(
                            client,
                            channel,
                            current_download_start,
                            future_ver,
                            &progress_callback,
                            cancel_token.clone(),
                        ).await {
                            Ok(pwr_path) => {
                                println!("Found larger patch: {}->{} (skipping missing intermediates)", current_download_start, future_ver);
                                patch_files.push((pwr_path, future_ver));
                                current_download_start = future_ver;
                                step_index = next_step_index + 1; // Skip all intermediate steps
                                found_larger_patch = true;
                                break;
                            }
                            Err(_) => {
                                next_step_index += 1;
                            }
                        }
                    }
                    
                    if !found_larger_patch {
                        println!("No patches available from {} to any later version", current_download_start);
                        break;
                    }
                }
            }
        }
    }

    // --- PHASE 2: INSTALL ALL PATCHES ---
    let mut current_ver = 0;  // Always start from 0 for installation
    for (idx, (pwr_path, next_ver)) in patch_files.iter().enumerate() {
        progress_callback(
            "install",
            ((idx as f32 / patch_files.len() as f32) * 100.0) as f64,
            &format!("Applying patch {}/{}: {} -> {}", idx + 1, patch_files.len(), current_ver, next_ver),
            (idx + 1) as u64,
            patch_files.len() as u64,
            None,
            Some(idx + 1), // Pasar el step actual (idx + 1)
        );

        crate::game::patcher::apply_pwr(
            base_dir,
            channel,
            &install_dir_name,
            pwr_path,
            &progress_callback,
            cancel_token.clone(),
        )
        .await?;

        current_ver = *next_ver;

        if is_latest {
            let _ = save_local_version(base_dir, channel, current_ver).await;
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

    progress_callback("complete", 100.0, "Game installed successfully", 0, 0, None, None);

    if start_version != target_ver_val {
        let _ = crate::game::patcher::clean_patches_cache(&progress_callback).await;
    }

    // Return the total number of steps for UI display
    let total_steps = if highest_complete_version > 0 {
        // If we had a complete version + incrementals
        let incremental_count = target_ver_val.saturating_sub(highest_complete_version);
        (1 + incremental_count) as usize // 1 for complete + number of incrementals
    } else {
        // If only incrementals from 0
        target_ver_val as usize
    };
    
    Ok((total_steps, ()))
}
