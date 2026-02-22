use anyhow::Result;
use std::path::PathBuf;
use super::PatchApiFrontend;
use crate::patch_api::GameVersionInfo;

impl PatchApiFrontend {
    /// Gets comprehensive version information
    /// Replaces: crate::game::patcher::get_version_manifest
    pub async fn get_version_info(
        &self,
        base_dir: &PathBuf,
        channel: &str,
        user_version: i32,
    ) -> Result<GameVersionInfo> {
        self.version_manager
            .get_version_info(base_dir, channel, user_version)
            .await
    }

    /// Finds the latest available game version
    /// Replaces: crate::game::patcher::find_latest_version
    pub async fn find_latest_version(&self, channel: &str, start_hint: Option<i32>) -> Result<i32> {
        self.version_manager
            .find_latest_version(channel, start_hint)
            .await
    }
}