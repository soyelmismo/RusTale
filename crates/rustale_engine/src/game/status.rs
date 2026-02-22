use rustale_shared::config::GameSettings;
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
    settings: &GameSettings,
    paths: &GamePaths,
    cached_remote_latest: Option<i32>,
) -> (LauncherStatus, Option<i32>) {
    let channel = &settings.channel;
    let is_latest_mode = settings.game_version == 0;

    println!(
        "[Status] Calculating status - channel: {}, is_latest: {}, cached: {:?}",
        channel, is_latest_mode, cached_remote_latest
    );

    // Determine version string for folder lookup
    let version_str = if is_latest_mode {
        "latest".to_string()
    } else {
        settings.game_version.to_string()
    };

    // Step 1: Check if game is installed locally
    let exe_path = paths.client_exe(channel, &version_str);
    println!("[Status] Checking game executable at: {}", exe_path.display());
    let is_installed = tokio::fs::metadata(&exe_path).await.is_ok();

    if !is_installed {
        println!("[Status] Game not installed at {}", exe_path.display());
        return (LauncherStatus::NeedsInstall, cached_remote_latest);
    }
    println!("[Status] Game executable found and accessible");

    // Step 2: Verify installation integrity
    let game_dir = paths.version_dir(channel, &version_str);
    println!("[Status] Checking integrity in directory: {}", game_dir.display());
    let integrity_checker = crate::game::patch_api::IntegrityChecker::new();
    if let Err(e) = integrity_checker.verify_extraction_integrity(&game_dir).await {
        println!("[Status] Integrity check failed: {}. Marked as NeedsInstall.", e);
        return (LauncherStatus::NeedsInstall, None);
    }
    println!("[Status] Installation integrity verified successfully");

    // Step 3: Check for updates (PURE LOGIC)
    if is_latest_mode {
        if let Some(remote_latest) = cached_remote_latest {
            let local_version = crate::game::get_local_version(&paths.root, channel).await.unwrap_or(0);
            println!("[Status] Version check - local: {}, remote: {}", local_version, remote_latest);
            if local_version < remote_latest {
                println!("[Status] Update available: {} -> {}", local_version, remote_latest);
                return (LauncherStatus::NeedsUpdate, Some(remote_latest));
            }
        } else {
            println!("[Status] Latest mode but no remote version cached");
        }
    }

    println!("[Status] Final status: Ready");
    (LauncherStatus::Ready, cached_remote_latest)
}
