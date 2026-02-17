use crate::config::GameSettings;
use crate::game::paths::GamePaths;

#[derive(Debug, Clone, PartialEq)]
pub enum LauncherStatus {
    Checking,
    Ready,        // Installed and ready to play
    NeedsUpdate,  // Installed but update available (only for latest mode)
    NeedsInstall, // Not installed
    Playing,
    Busy,
    Downloading,
    Migrating,
}

/// Calculates current launcher status based on settings and file system state
/// This function implements core verification logic:
/// 1. Check if user has game installed for selected channel/version
/// 2. If "latest" mode, check local version vs remote version
/// 3. If update available, return NeedsUpdate
/// 4. If specific version selected, just verify it exists
/// 5. ENHANCED: Verify installation integrity, not just file existence
///
/// Returns: (Status, Optional<remote_version>)
pub async fn calculate_status(
    client: &reqwest::Client,
    settings: &GameSettings,
    paths: &GamePaths,
    cached_remote_latest: Option<i32>,
) -> (LauncherStatus, Option<i32>) {
    let channel = &settings.channel;
    let is_latest_mode = settings.game_version == 0;

    println!(
        "[DEBUG] calculate_status - channel: {}, is_latest_mode: {}, cached: {:?}",
        channel, is_latest_mode, cached_remote_latest
    );

    // Determine version string for folder lookup
    let version_str = if is_latest_mode {
        "latest".to_string()
    } else {
        settings.game_version.to_string()
    };

    // Step 2: Check if game is installed locally
    let exe_path = paths.client_exe(channel, &version_str);

    let is_installed = tokio::fs::metadata(&exe_path).await.is_ok();
    println!(
        "[DEBUG] Game installed at {}: {}",
        exe_path.display(),
        is_installed
    );

    if !is_installed {
        // Game not installed at all
        println!("[DEBUG] Game not installed, returning NeedsInstall");
        return (LauncherStatus::NeedsInstall, None);
    }

    // ENHANCED: Verify installation integrity for installed games
    if is_installed {
        let game_dir = paths.version_dir(channel, &version_str);
        let integrity_checker = crate::game::patch_api::IntegrityChecker::new();
        match integrity_checker
            .verify_extraction_integrity(&game_dir)
            .await
        {
            Ok(_) => {
                println!("[DEBUG] Installation integrity verified");
            }
            Err(e) => {
                println!("[DEBUG] Installation integrity check failed: {}", e);
                // Installation exists but is corrupted, needs reinstall
                return (LauncherStatus::NeedsInstall, None);
            }
        }
    }

    // If not in latest mode, and it's installed and verified, we're ready
    if !is_latest_mode {
        println!("[DEBUG] Not latest mode and installed verified, returning Ready");
        return (LauncherStatus::Ready, None);
    }

    // LOGICA DE CACHE
    // Si ya tenemos el dato remoto (del inicio del launcher), lo usamos.
    if let Some(remote_ver) = cached_remote_latest {
        // Leemos la version local del json
        let version_file = paths.version_json(channel);
        let local_ver = if let Ok(content) = tokio::fs::read_to_string(&version_file).await {
            serde_json::from_str::<serde_json::Value>(&content)
                .ok()
                .and_then(|v| v.get("version").and_then(|n| n.as_i64()))
                .unwrap_or(0) as i32
        } else {
            0
        };

        if local_ver < remote_ver {
            return (LauncherStatus::NeedsUpdate, Some(remote_ver));
        } else {
            return (LauncherStatus::Ready, Some(remote_ver));
        }
    }

    // Step 3: Latest mode - check for updates using PatchApiFrontend
    let version_info = crate::game::patch_api::PatchApiFrontend::get_instance()
        .get_version_info(&paths.root, channel, settings.game_version as i32)
        .await;

    let version_info = match version_info {
        Ok(info) => info,
        Err(_) => {
            // If network fails, allow offline Ready if installed
            return (LauncherStatus::Ready, None);
        }
    };

    if version_info.update_available {
        (
            LauncherStatus::NeedsUpdate,
            Some(version_info.latest_remote),
        )
    } else {
        (LauncherStatus::Ready, Some(version_info.latest_remote))
    }
}
