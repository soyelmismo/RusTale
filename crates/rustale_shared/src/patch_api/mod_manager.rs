use anyhow::Result;
use std::sync::Arc;

use crate::patch_api::traits::PatchProvider;
#[cfg(feature = "security")]
use crate::patch_api::providers::get_all_providers;

// ============================================================================
// UTILITY TYPES
// ============================================================================

/// Provider version information for comparing across providers
#[derive(Debug, Clone)]
pub struct ProviderVersionInfo {
    pub provider_name: String,
    pub provider_priority: i32,
    pub version: i32,
    pub channel: String,
}

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

/// Normaliza nombres de arquitectura
pub fn normalize_architecture(arch: &str) -> String {
    match arch {
        "x86_64" => "amd64".to_string(),
        "aarch64" => "arm64".to_string(),
        _ => arch.to_string(),
    }
}

/// Normaliza nombres de sistema operativo
pub fn normalize_os(os: &str) -> String {
    match os {
        "darwin" => "mac".to_string(),
        _ => os.to_string(),
    }
}

/// Extrae número de versión de string como "v11" o "0_to_11"
pub fn extract_version_number(version: &str) -> i32 {
    if let Some(to_match) = version.find("_to_") {
        version[to_match + 4..].parse().unwrap_or(0)
    } else if version.starts_with('v') {
        version[1..].parse().unwrap_or(0)
    } else {
        version.parse().unwrap_or(0)
    }
}

// ============================================================================
// PATCH API MANAGER
// ============================================================================

/// Generic patch API manager that handles multiple providers
pub struct PatchApiManager {
    providers: Vec<Arc<dyn PatchProvider>>,
}

impl PatchApiManager {
    pub fn new() -> Self {
        #[cfg(feature = "security")]
        let providers = get_all_providers();
        
        #[cfg(not(feature = "security"))]
        let providers: Vec<Arc<dyn PatchProvider>> = Vec::new();

        Self {
            providers,
        }
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

    /// Try to get the latest version from any provider
    pub async fn get_latest_version(&self, channel: &str, os: &str, arch: &str) -> Result<i32> {
        for provider in &self.providers {
            if !provider.is_available().await {
                continue;
            }

            if let Ok(version) = provider.get_latest_version(channel, os, arch).await {
                return Ok(version);
            }
        }
        anyhow::bail!("No provider could fetch the latest version")
    }

    /// Find the latest version across ALL providers
    pub async fn find_latest_version_across_providers(
        &self,
        channel: &str,
        os: &str,
        arch: &str,
    ) -> Result<ProviderVersionInfo> {
        let mut best: Option<ProviderVersionInfo> = None;

        for provider in &self.providers {
            if !provider.is_available().await {
                continue;
            }

            if let Ok(version) = provider.get_latest_version(channel, os, arch).await {
                let info = ProviderVersionInfo {
                    provider_name: provider.name().to_string(),
                    provider_priority: provider.priority(),
                    version,
                    channel: channel.to_string(),
                };

                if best.is_none() || version > best.as_ref().unwrap().version {
                    best = Some(info);
                }
            }
        }

        best.ok_or_else(|| anyhow::anyhow!("No provider available"))
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

    pub fn get_provider_names(&self) -> Vec<&str> {
        self.providers.iter().map(|p| p.name()).collect()
    }
}

impl Default for PatchApiManager {
    fn default() -> Self {
        Self::new()
    }
}