use crate::patch_api::traits::PatchProvider;
#[cfg(feature = "security")]
use crate::patch_api::zp::ZProvider;
use anyhow::Result;
use std::sync::Arc;
//use crate::patch_api::shipofyarn::ShipOfYarnProvider;

/// Generic patch API manager that handles multiple providers
pub struct PatchApiManager {
    providers: Vec<Arc<dyn PatchProvider>>,
}

impl PatchApiManager {
    pub fn new() -> Self {
        let mut providers: Vec<Arc<dyn PatchProvider>> = Vec::new();

        // Automatically add all available providers in order of preference
        #[cfg(feature = "security")]
        providers.push(Arc::new(ZProvider::new()));
        //manager.providers.push(Arc::new(ShipOfYarnProvider::new()));

        Self { providers }
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
    ) -> Result<zeroize::Zeroizing<String>> {
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
        println!(
            "[PatchApiManager] Trying {} providers",
            self.providers.len()
        );

        for (i, provider) in self.providers.iter().enumerate() {
            println!(
                "[PatchApiManager] Testing provider {}: {}",
                i,
                provider.name()
            );

            // Check provider availability with timeout
            let is_available = match tokio::time::timeout(
                std::time::Duration::from_secs(5),
                provider.is_available(),
            )
            .await
            {
                Ok(available) => {
                    println!(
                        "[PatchApiManager] Provider {} availability check completed",
                        i
                    );
                    available
                }
                Err(_) => {
                    println!(
                        "[PatchApiManager] Provider {} availability timeout, skipping",
                        i
                    );
                    continue;
                }
            };

            println!(
                "[PatchApiManager] Provider {} availability: {}",
                i, is_available
            );

            if !is_available {
                println!("[PatchApiManager] Provider {} not available", i);
                continue;
            }

            println!(
                "[PatchApiManager] Provider {} available, fetching latest version",
                i
            );

            // Try to fetch version with timeout, but don't fail the entire operation
            let version_result = tokio::time::timeout(
                std::time::Duration::from_secs(6),
                provider.get_latest_version(channel, os, arch),
            )
            .await;

            match version_result {
                Ok(Ok(version)) => {
                    println!(
                        "[PatchApiManager] Provider {} returned version: {}",
                        i, version
                    );
                    return Ok(version);
                }
                Ok(Err(e)) => {
                    println!("[PatchApiManager] Provider {} failed: {}", i, e);
                    continue;
                }
                Err(_) => {
                    println!(
                        "[PatchApiManager] Provider {} timeout, trying next provider",
                        i
                    );
                    continue;
                }
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
            if !provider.is_available().await {
                continue;
            }

            if let Ok(versions) = provider.get_available_versions(channel, os, arch).await {
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
    ) -> Result<zeroize::Zeroizing<String>> {
        for provider in &self.providers {
            if !provider.is_available().await {
                continue;
            }

            if let Ok(url) = provider
                .get_patch_url(channel, os, arch, from_version, to_version)
                .await
            {
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
            if !provider.is_available().await {
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
