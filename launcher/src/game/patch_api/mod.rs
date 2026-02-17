pub mod estrogen;
pub mod frontend;
pub mod installer;
pub mod integrity_checker;
pub mod patch_downloader;
pub mod shared_cache;
pub mod shipofyarn;
pub mod traits;
pub mod utils;
pub mod version_manager;

pub use estrogen::EstrogenProvider;
pub use frontend::PatchApiFrontend;
pub use installer::{ButlerInstaller, JreInstaller};
pub use integrity_checker::IntegrityChecker;
pub use patch_downloader::PatchDownloader;
pub use shared_cache::get_shared_cache;
pub use shipofyarn::ShipOfYarnProvider;
pub use traits::PatchProvider;
pub use utils::*;
pub use version_manager::VersionManager;

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

    /// Static helper for getting latest version
    pub async fn get_latest_version_static(channel: &str, os: &str, arch: &str) -> Result<i32> {
        let manager = Self::new();
        manager.get_latest_version(channel, os, arch).await
    }

    /// Static helper for getting available versions
    pub async fn get_available_versions_static(
        channel: &str,
        os: &str,
        arch: &str,
    ) -> Result<Vec<i32>> {
        let manager = Self::new();
        manager.get_available_versions(channel, os, arch).await
    }

    /// Static helper for getting patch URL
    pub async fn get_patch_url_static(
        channel: &str,
        os: &str,
        arch: &str,
        from_version: i32,
        to_version: i32,
    ) -> Result<String> {
        let manager = Self::new();
        manager
            .get_patch_url(channel, os, arch, from_version, to_version)
            .await
    }

    /// Static helper for checking complete version
    pub async fn has_complete_version_static(
        channel: &str,
        os: &str,
        arch: &str,
        version: i32,
    ) -> bool {
        let manager = Self::new();
        manager
            .has_complete_version(channel, os, arch, version)
            .await
    }

    /// Try to get the latest version from any provider
    pub async fn get_latest_version(&self, channel: &str, os: &str, arch: &str) -> Result<i32> {
        for provider in &self.providers {
            // Check if provider is available before attempting API calls
            if !provider.is_available().await {
                println!(
                    "⚠️  Provider {} is not available, skipping",
                    provider.name()
                );
                continue;
            }

            if let Ok(version) = provider.get_latest_version(channel, os, arch).await {
                println!(
                    "✅ Successfully got latest version {} from provider: {}",
                    version,
                    provider.name()
                );
                return Ok(version);
            }
        }
        anyhow::bail!("No provider could fetch the latest version")
    }

    /// Try to get all available versions from any provider
    pub async fn get_available_versions(
        &self,
        channel: &str,
        os: &str,
        arch: &str,
    ) -> Result<Vec<i32>> {
        for provider in &self.providers {
            // Check if provider is available before attempting API calls
            if !provider.is_available().await {
                println!(
                    "⚠️  Provider {} is not available, skipping",
                    provider.name()
                );
                continue;
            }

            if let Ok(versions) = provider.get_available_versions(channel, os, arch).await {
                println!(
                    "✅ Successfully got {} versions from provider: {}",
                    versions.len(),
                    provider.name()
                );
                return Ok(versions);
            }
        }
        anyhow::bail!("No provider could fetch available versions")
    }

    /// Try to get patch download URL from any provider
    pub async fn get_patch_url(
        &self,
        channel: &str,
        os: &str,
        arch: &str,
        from_version: i32,
        to_version: i32,
    ) -> Result<String> {
        for provider in &self.providers {
            // Check if provider is available before attempting API calls
            if !provider.is_available().await {
                println!(
                    "⚠️  Provider {} is not available, skipping",
                    provider.name()
                );
                continue;
            }

            if let Ok(url) = provider
                .get_patch_url(channel, os, arch, from_version, to_version)
                .await
            {
                println!(
                    "✅ Successfully got patch URL from provider: {}",
                    provider.name()
                );
                return Ok(url);
            }
        }
        anyhow::bail!(
            "No provider could fetch patch URL for {}->{}",
            from_version,
            to_version
        )
    }

    /// Check if a complete version exists from any provider
    pub async fn has_complete_version(
        &self,
        channel: &str,
        os: &str,
        arch: &str,
        version: i32,
    ) -> bool {
        for provider in &self.providers {
            // Check if provider is available before attempting API calls
            if !provider.is_available().await {
                println!(
                    "⚠️  Provider {} is not available, skipping",
                    provider.name()
                );
                continue;
            }

            if let Ok(has_version) = provider
                .has_complete_version(channel, os, arch, version)
                .await
            {
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
