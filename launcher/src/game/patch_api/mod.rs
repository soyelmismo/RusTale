pub mod estrogen;
pub mod hytale;
pub mod shipofyarn;
pub mod traits;
pub mod utils;
pub mod installer;
pub mod version_manager;
pub mod patch_downloader;
pub mod integrity_checker;
pub mod frontend;
pub mod compat;
pub mod shared_cache;
pub mod example;

pub use traits::{PatchProvider, VersionInfo, PatchInfo, DownloadInfo};
pub use estrogen::EstrogenProvider;
pub use hytale::HytaleProvider;
pub use shipofyarn::ShipOfYarnProvider;
pub use utils::*;
pub use installer::{ButlerInstaller, JreInstaller};
pub use version_manager::VersionManager;
pub use patch_downloader::PatchDownloader;
pub use integrity_checker::{IntegrityChecker, IntegrityResult, FormatValidationResult};
pub use frontend::PatchApiFrontend;
pub use compat::*;
pub use shared_cache::{SharedCacheManager, init_shared_cache, get_shared_cache, CacheStats};

use anyhow::Result;
use std::sync::Arc;

/// Generic patch API manager that handles multiple providers
pub struct PatchApiManager {
    providers: Vec<Arc<dyn PatchProvider>>,
}

impl PatchApiManager {
    pub fn new() -> Self {
        let mut manager = Self {
            providers: Vec::new(),
        };
        
        // Automatically add all available providers in order of preference
        manager.providers.push(Arc::new(EstrogenProvider::new()));
        manager.providers.push(Arc::new(ShipOfYarnProvider::new()));
        // Note: HytaleProvider requires authentication tokens and should be added manually if needed
        
        manager
    }

    /// Try to get the latest version from any provider
    pub async fn get_latest_version(&self, channel: &str, os: &str, arch: &str) -> Result<i32> {
        for provider in &self.providers {
            if let Ok(version) = provider.get_latest_version(channel, os, arch).await {
                return Ok(version);
            }
        }
        anyhow::bail!("No provider could fetch the latest version")
    }

    /// Try to get all available versions from any provider
    pub async fn get_available_versions(&self, channel: &str, os: &str, arch: &str) -> Result<Vec<i32>> {
        for provider in &self.providers {
            if let Ok(versions) = provider.get_available_versions(channel, os, arch).await {
                return Ok(versions);
            }
        }
        anyhow::bail!("No provider could fetch available versions")
    }

    /// Try to get patch download URL from any provider
    pub async fn get_patch_url(&self, channel: &str, os: &str, arch: &str, from_version: i32, to_version: i32) -> Result<String> {
        for provider in &self.providers {
            if let Ok(url) = provider.get_patch_url(channel, os, arch, from_version, to_version).await {
                return Ok(url);
            }
        }
        anyhow::bail!("No provider could fetch patch URL for {}->{}", from_version, to_version)
    }

    /// Try to get patch signature URL from any provider
    pub async fn get_patch_signature_url(&self, channel: &str, os: &str, arch: &str, from_version: i32, to_version: i32) -> Result<String> {
        for provider in &self.providers {
            if let Ok(url) = provider.get_patch_signature_url(channel, os, arch, from_version, to_version).await {
                return Ok(url);
            }
        }
        anyhow::bail!("No provider could fetch patch signature URL for {}->{}", from_version, to_version)
    }

    /// Try to get JRE download URL from any provider
    pub async fn get_jre_url(&self, os: &str, arch: &str) -> Result<String> {
        // First try the providers
        for provider in &self.providers {
            if let Ok(url) = provider.get_jre_url(os, arch).await {
                return Ok(url);
            }
        }
        
        // If all providers fail, use Adoptium fallback
        println!("All providers failed for JRE, using Adoptium fallback");
        Ok(get_java_adoptium_url(os, arch))
    }

    /// Try to get Butler download URL from any provider
    pub async fn get_butler_url(&self, os: &str, arch: &str) -> Result<String> {
        // First try the providers
        for provider in &self.providers {
            if let Ok(url) = provider.get_butler_url(os, arch).await {
                return Ok(url);
            }
        }
        
        // If all providers fail, use the itch.io CDN fallback
        println!("All providers failed for Butler, using itch.io CDN fallback");
        Ok(get_butler_fallback_url(os, arch))
    }

    /// Check if a complete version exists from any provider
    pub async fn has_complete_version(&self, channel: &str, os: &str, arch: &str, version: i32) -> bool {
        for provider in &self.providers {
            if let Ok(has_version) = provider.has_complete_version(channel, os, arch, version).await {
                if has_version {
                    return true;
                }
            }
        }
        false
    }
}

impl Default for PatchApiManager {
    fn default() -> Self {
        Self::new()
    }
}
