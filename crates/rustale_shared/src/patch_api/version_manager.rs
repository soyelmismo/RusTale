use anyhow::Result;
use std::path::PathBuf;

use crate::patch_api::mod_manager::PatchApiManager;
use crate::patch_api::utils::get_arch_name;
use crate::patch_api::types::{GameVersionInfo, get_local_version};

/// Manager for game version operations
#[derive(Clone)]
pub struct VersionManager {}

impl VersionManager {
    pub fn new() -> Self {
        Self {}
    }

    /// Gets comprehensive version information for a channel
    pub async fn get_version_info(
        &self,
        base_dir: &PathBuf,
        channel: &str,
        user_version: i32,
    ) -> Result<GameVersionInfo> {
        let local_version = get_local_version(base_dir, channel)
            .await
            .unwrap_or(0);

        // Get latest version from patch API
        let latest = PatchApiManager::get_latest_version_static(
            channel,
            std::env::consts::OS,
            get_arch_name(),
        )
        .await?;

        // Generate default list
        let mut available: Vec<i32> = (1..=latest).collect();
        available.reverse();

        // Try to get real available versions
        let available_versions_from_fallback = match PatchApiManager::get_available_versions_static(
            channel,
            std::env::consts::OS,
            get_arch_name(),
        )
        .await
        {
            Ok(versions) => Some(versions),
            Err(_) => None,
        };

        Ok(GameVersionInfo {
            user_version,
            current_local: local_version,
            latest_remote: latest,
            available_versions: available,
            available_versions_from_fallback,
            update_available: user_version == 0 && local_version < latest,
        })
    }

    /// Finds the latest available game version
    pub async fn find_latest_version(&self, channel: &str, start_hint: Option<i32>) -> Result<i32> {
        let os = std::env::consts::OS;
        let arch = get_arch_name();

        if let Some(hint) = start_hint {
            if hint > 0
                && PatchApiManager::has_complete_version_static(channel, os, arch, hint).await
            {
                return Ok(hint);
            }
        }

        PatchApiManager::get_latest_version_static(channel, os, arch).await
    }
}
