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

/// Calculates the current launcher status based on settings and file system state
/// This function implements the core verification logic:
/// 1. Check if user has game installed for selected channel/version
/// 2. If "latest" mode, check local version vs remote version
/// 3. If update available, return NeedsUpdate
/// 4. If specific version selected, just verify it exists
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

    // Determine the version string for folder lookup
    let version_str = if is_latest_mode {
        "latest".to_string()
    } else {
        settings.game_version.to_string()
    };

    // Step 2: Check if game is installed locally
    let exe_path = paths.client_exe(channel, &version_str);

    let is_installed = tokio::fs::metadata(&exe_path).await.is_ok();

    if !is_installed {
        // Game not installed at all
        return (LauncherStatus::NeedsInstall, None);
    }

    // If not in latest mode, and it's installed, we're ready
    if !is_latest_mode {
        return (LauncherStatus::Ready, None);
    }

    // LOGICA DE CACHE
    // Si ya tenemos el dato remoto (del inicio del launcher), lo usamos.
    if let Some(remote_ver) = cached_remote_latest {
        // Leemos la versión local del json
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

    // Step 3: Latest mode - check for updates
    match crate::game::patcher::get_version_manifest(
        client,
        channel,
        &paths.root,
        settings.game_version as i32,
    )
    .await
    {
        Ok(manifest) => {
            if manifest.update_available {
                (LauncherStatus::NeedsUpdate, Some(manifest.latest_remote))
            } else {
                (LauncherStatus::Ready, Some(manifest.latest_remote))
            }
        }
        Err(_) => {
            // Can't reach server, but game is installed
            // Read local version just to have something
            let version_file = paths.version_json(channel);
            let local_ver = if let Ok(content) = tokio::fs::read_to_string(&version_file).await {
                serde_json::from_str::<serde_json::Value>(&content)
                    .ok()
                    .and_then(|v| v.get("version").and_then(|n| n.as_i64()))
                    .unwrap_or(0) as i32
            } else {
                0
            };
            (LauncherStatus::Ready, Some(local_ver))
        }
    }
}
