use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

#[async_trait]
pub trait PatchProvider: Send + Sync {
    /// Name of the provider (e.g., "E", "S", "H1", "V")
    fn name(&self) -> &str;

    /// Priority of the provider (higher is preferred)
    fn priority(&self) -> i32;

    /// Check if the provider is currently available (online)
    async fn is_available(&self) -> bool;

    /// Get the latest version available on a channel
    async fn get_latest_version(&self, channel: &str, os: &str, arch: &str) -> Result<i32>;

    /// Get all available versions on a channel
    async fn get_available_versions(&self, channel: &str, os: &str, arch: &str)
    -> Result<Vec<i32>>;

    /// Get the download URL for a patch from->to
    async fn get_patch_url(
        &self,
        channel: &str,
        os: &str,
        arch: &str,
        from_version: i32,
        to_version: i32,
    ) -> Result<zeroize::Zeroizing<String>>;

    /// Check if a complete version download is available
    async fn has_complete_version(
        &self,
        channel: &str,
        os: &str,
        arch: &str,
        version: i32,
    ) -> Result<bool>;

    /// Get the download URL for a complete version
    async fn get_complete_url(
        &self,
        _channel: &str,
        _os: &str,
        _arch: &str,
        _version: i32,
    ) -> Result<zeroize::Zeroizing<String>> {
        anyhow::bail!("Complete versions not supported on this provider")
    }

    /// Download a patch directly to disk using Zero-Trace secure download.
    /// This method downloads data in chunks that are immediately written to disk
    /// and zeroized from memory, preventing sensitive URLs from lingering in RAM.
    /// 
    /// # Arguments
    /// * `channel` - Release channel (e.g., "release", "beta")
    /// * `os` - Operating system
    /// * `arch` - Architecture
    /// * `from_version` - Starting version
    /// * `to_version` - Target version
    /// * `dest_path` - Destination file path
    /// * `cancel_token` - Token to cancel the download
    /// * `progress_callback` - Callback for progress updates (percentage, total, downloaded)
    #[cfg(feature = "security")]
    async fn download_patch_secure(
        &self,
        channel: &str,
        os: &str,
        arch: &str,
        from_version: i32,
        to_version: i32,
        dest_path: &Path,
        cancel_token: Arc<AtomicBool>,
        progress_callback: Box<dyn Fn(f64, u64, u64) + Send + Sync>,
    ) -> Result<()>;
}
