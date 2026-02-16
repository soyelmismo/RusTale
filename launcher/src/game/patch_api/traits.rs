use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use anyhow::Result;

/// Information about a version
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    pub version: i32,
    pub channel: String,
    pub is_complete: bool,
}

/// Information about a patch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchInfo {
    pub from_version: i32,
    pub to_version: i32,
    pub channel: String,
    pub is_complete: bool,
    pub download_url: String,
    pub signature_url: Option<String>,
}

/// Download information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadInfo {
    pub url: String,
    pub filename: String,
    pub size: Option<u64>,
    pub checksum: Option<String>,
}

/// Generic trait for patch providers
#[async_trait]
pub trait PatchProvider: Send + Sync {
    /// Get the provider name
    fn name(&self) -> &'static str;

    /// Check if the provider is available
    async fn is_available(&self) -> bool;

    /// Get the latest version for a channel
    async fn get_latest_version(&self, channel: &str, os: &str, arch: &str) -> Result<i32>;

    /// Get all available versions for a channel
    async fn get_available_versions(&self, channel: &str, os: &str, arch: &str) -> Result<Vec<i32>>;

    /// Get patch download URL
    async fn get_patch_url(&self, channel: &str, os: &str, arch: &str, from_version: i32, to_version: i32) -> Result<String>;

    /// Get patch signature URL (optional)
    async fn get_patch_signature_url(&self, channel: &str, os: &str, arch: &str, from_version: i32, to_version: i32) -> Result<String>;

    /// Check if a complete version exists
    async fn has_complete_version(&self, channel: &str, os: &str, arch: &str, version: i32) -> Result<bool>;

    /// Get JRE download URL
    async fn get_jre_url(&self, os: &str, arch: &str) -> Result<String>;

    /// Get Butler download URL (optional)
    async fn get_butler_url(&self, os: &str, arch: &str) -> Result<String> {
        anyhow::bail!("Butler downloads not supported by this provider")
    }
}
