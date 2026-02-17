use anyhow::Result;
use async_trait::async_trait;

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
    async fn get_available_versions(&self, channel: &str, os: &str, arch: &str)
    -> Result<Vec<i32>>;

    /// Get patch download URL
    async fn get_patch_url(
        &self,
        channel: &str,
        os: &str,
        arch: &str,
        from_version: i32,
        to_version: i32,
    ) -> Result<String>;

    /// Check if a complete version exists
    async fn has_complete_version(
        &self,
        channel: &str,
        os: &str,
        arch: &str,
        version: i32,
    ) -> Result<bool>;
}
