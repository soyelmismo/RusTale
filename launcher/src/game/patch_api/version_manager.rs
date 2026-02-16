use anyhow::{Context, Result};
use std::path::PathBuf;

use super::PatchApiManager;
use crate::game::patcher::GameVersionInfo;

/// Manager for game version operations using the new patch API system
pub struct VersionManager {
    api_manager: Arc<PatchApiManager>,
}

impl VersionManager {
    pub fn new(api_manager: Arc<PatchApiManager>) -> Self {
        Self { api_manager }
    }

    /// Gets comprehensive version information for a channel
    pub async fn get_version_info(
        &self,
        client: &reqwest::Client,
        base_dir: &PathBuf,
        channel: &str,
        user_version: i32,
    ) -> Result<GameVersionInfo> {
        let local_version = crate::game::install::get_local_version(base_dir, channel)
            .await
            .unwrap_or(0);

        // Get latest version from patch API
        let latest = self.api_manager.get_latest_version(channel, std::env::consts::OS, get_arch_name()).await?;

        // Generate default list (assuming all versions exist)
        let mut available: Vec<i32> = (1..=latest).collect();
        available.reverse(); // From newest to oldest for the UI

        // Try to get real available versions from patch API
        let available_versions_from_fallback = match self.api_manager.get_available_versions(channel, std::env::consts::OS, get_arch_name()).await {
            Ok(versions) => {
                println!("Using {} versions from patch API for channel {}", versions.len(), channel);
                Some(versions)
            }
            Err(e) => {
                println!("Failed to get versions from patch API: {}", e);
                None
            }
        };

        Ok(GameVersionInfo {
            user_version,
            current_local: local_version,
            latest_remote: latest,
            available_versions: available,
            available_versions_from_fallback,
            // update if user uses 0 (latest) and its local installed version is lower than latest remote
            update_available: user_version == 0 && local_version < latest,
        })
    }

    /// Finds the latest available game version using the patch API
    pub async fn find_latest_version(
        &self,
        channel: &str,
        start_hint: Option<i32>,
    ) -> Result<i32> {
        let os = std::env::consts::OS;
        let arch = get_arch_name();

        // If we have a hint, try it first
        if let Some(hint) = start_hint {
            if hint > 0 && self.api_manager.has_complete_version(channel, os, arch, hint).await {
                println!("Starting from hint: version {}", hint);
                return Ok(hint);
            }
        }

        // Get latest version from patch API
        self.api_manager.get_latest_version(channel, os, arch).await
    }

    /// Gets all available versions for a channel
    pub async fn get_available_versions(&self, channel: &str) -> Result<Vec<i32>> {
        let os = std::env::consts::OS;
        let arch = get_arch_name();
        
        self.api_manager.get_available_versions(channel, os, arch).await
    }

    /// Checks if a specific version exists
    pub async fn version_exists(&self, channel: &str, version: i32) -> bool {
        let os = std::env::consts::OS;
        let arch = get_arch_name();
        
        self.api_manager.has_complete_version(channel, os, arch, version).await
    }

    /// Gets patch download URL for a version range
    pub async fn get_patch_url(
        &self,
        channel: &str,
        from_version: i32,
        to_version: i32,
    ) -> Result<String> {
        let os = std::env::consts::OS;
        let arch = get_arch_name();
        
        self.api_manager.get_patch_url(channel, os, arch, from_version, to_version).await
    }

    /// Gets patch signature URL for a version range
    pub async fn get_patch_signature_url(
        &self,
        channel: &str,
        from_version: i32,
        to_version: i32,
    ) -> Result<String> {
        let os = std::env::consts::OS;
        let arch = get_arch_name();
        
        self.api_manager.get_patch_signature_url(channel, os, arch, from_version, to_version).await
    }
}

fn get_arch_name() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    }
}
