use crate::core::signals::FromCore;
use crate::game::patch_api::PatchApiFrontend;
use crate::game::paths::GamePaths;
use anyhow::{Context, Result};
use tokio::sync::mpsc;

pub struct VersionService {
    paths: GamePaths,
}

impl VersionService {
    pub fn new(root_dir: std::path::PathBuf) -> Self {
        Self {
            paths: GamePaths::new(root_dir),
        }
    }

    /// Fetches remote versions, caches them, and broadcasts to UI.
    /// Returns the latest version number found.
    pub async fn refresh_versions(
        &self,
        channel: &str,
        tx: &mpsc::Sender<FromCore>,
    ) -> Result<i32> {
        let frontend = PatchApiFrontend::get_instance();
        
        println!("[VersionService] Fetching latest version for channel: {}", channel);

        // 1. Fetch Latest - let PatchApiManager handle provider timeouts individually
        let latest = frontend.find_latest_version(channel, None).await
            .context("Failed to query patch API")?;

        println!("[VersionService] Remote latest: {}", latest);

        // 2. Generate Version List
        // Logic logic: Generate 50 versions back from latest
        let mut versions = Vec::new();
        for i in (1..=latest).rev().take(50) {
            versions.push(i);
        }

        // 3. Broadcast to UI
        if let Err(e) = tx.send(FromCore::VersionCacheUpdated(versions)).await {
            println!("[VersionService] Failed to send version cache: {}", e);
            return Err(anyhow::anyhow!("Failed to broadcast versions: {}", e));
        }
        
        println!("[VersionService] Versions broadcasted successfully");
        Ok(latest)
    }

    /// Scans disk for installed versions and broadcasts.
    pub async fn scan_local_versions(&self, channel: &str, tx: &mpsc::Sender<FromCore>) -> Result<()> {
        let installed = crate::game::get_installed_versions(&self.paths.root, channel).await;
        tx.send(FromCore::InstalledVersionsLoaded(installed)).await?;
        Ok(())
    }
}
