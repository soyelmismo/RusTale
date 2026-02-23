use anyhow::Result;
use zeroize::Zeroizing;
use std::sync::Arc;

use crate::patch_api::traits::PatchProvider;
// Use the provider registry instead of importing individual providers
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

/// Construye la ruta de archivo para el manifest
pub fn build_manifest_path(os: &str, arch: &str, channel: &str) -> String {
    format!(
        "{}/{}/{}/manifest.json",
        normalize_os(os),
        normalize_architecture(arch),
        channel
    )
}

/// Construye la ruta de archivo para un patch específico
pub fn build_patch_path(
    os: &str,
    arch: &str,
    channel: &str,
    from_version: i32,
    to_version: i32,
) -> String {
    format!(
        "{}/{}/{}/{}_to_{}.pwr",
        normalize_os(os),
        normalize_architecture(arch),
        channel,
        from_version,
        to_version
    )
}

// ============================================================================
// PATCH API MANAGER
// ============================================================================

/// Generic patch API manager that handles multiple providers
/// 
/// Providers are obtained from the registry and are automatically ordered by priority.
pub struct PatchApiManager {
    providers: Vec<Arc<dyn PatchProvider>>,
}

impl PatchApiManager {
    pub fn new() -> Self {
        // Use the provider registry (plug-and-play)
        // Providers are automatically sorted by priority
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

    /// Static helper for getting patch URL
    pub async fn get_patch_url_static(
        channel: &str,
        os: &str,
        arch: &str,
        from_version: i32,
        to_version: i32,
    ) -> Result<Zeroizing<String>> {
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
    /// This method tries each provider in priority order and returns the first successful result
    pub async fn get_latest_version(&self, channel: &str, os: &str, arch: &str) -> Result<i32> {
        println!(
            "[PatchApiManager] Trying {} providers for channel '{}'",
            self.providers.len(),
            channel
        );

        for (i, provider) in self.providers.iter().enumerate() {
            println!(
                "[PatchApiManager] Testing provider {}: {} (priority: {})",
                i,
                provider.name(),
                provider.priority()
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

            if !is_available {
                println!("[PatchApiManager] Provider {} not available", i);
                continue;
            }

            // Try to fetch version with timeout
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

        anyhow::bail!("No provider could fetch the latest version for channel '{}'", channel)
    }

    /// Find the latest version across ALL providers, comparing results
    /// This queries all available providers and returns the highest version found
    /// Useful when some mirrors are outdated
    pub async fn find_latest_version_across_providers(
        &self,
        channel: &str,
        os: &str,
        arch: &str,
    ) -> Result<ProviderVersionInfo> {
        println!(
            "[PatchApiManager] Finding latest version across all providers for channel '{}'",
            channel
        );

        let mut results: Vec<ProviderVersionInfo> = Vec::new();
        let mut tasks = Vec::new();

        // Query all providers concurrently
        for provider in self.providers.iter() {
            let provider_name = provider.name().to_string();
            let provider_priority = provider.priority();

            let task = async move {
                // Check availability
                let is_available = match tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    provider.is_available(),
                )
                .await
                {
                    Ok(available) => available,
                    Err(_) => return None,
                };

                if !is_available {
                    return None;
                }

                // Get version
                let version_result = tokio::time::timeout(
                    std::time::Duration::from_secs(6),
                    provider.get_latest_version(channel, os, arch),
                )
                .await;

                match version_result {
                    Ok(Ok(version)) => Some(ProviderVersionInfo {
                        provider_name,
                        provider_priority,
                        version,
                        channel: channel.to_string(),
                    }),
                    _ => None,
                }
            };

            tasks.push(task);
        }

        // Collect results
        for task in tasks {
            if let Some(info) = task.await {
                println!(
                    "[PatchApiManager] Provider '{}' reports version {}",
                    info.provider_name, info.version
                );
                results.push(info);
            }
        }

        if results.is_empty() {
            anyhow::bail!(
                "No provider could fetch the latest version for channel '{}'",
                channel
            );
        }

        // Sort by version (descending), then by priority (descending)
        results.sort_by(|a, b| {
            b.version.cmp(&a.version)
                .then_with(|| b.provider_priority.cmp(&a.provider_priority))
        });

        let best = results.into_iter().next().unwrap();
        println!(
            "[PatchApiManager] Best version found: {} from provider '{}' (priority {})",
            best.version, best.provider_name, best.provider_priority
        );

        Ok(best)
    }

    /// Check if a specific channel is available on a provider
    pub async fn is_channel_available(&self, provider_name: &str, channel: &str) -> bool {
        for provider in &self.providers {
            if provider.name() == provider_name {
                // Try to get available versions for this channel
                if let Ok(true) = tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    async {
                        if !provider.is_available().await {
                            return false;
                        }
                        // If we can get versions, the channel exists
                        provider
                            .get_available_versions(
                                channel,
                                std::env::consts::OS,
                                std::env::consts::ARCH,
                            )
                            .await
                            .map(|v| !v.is_empty())
                            .unwrap_or(false)
                    }
                )
                .await
                {
                    return true;
                }
            }
        }
        false
    }

    /// Get all providers that support a specific channel
    pub async fn get_providers_for_channel(&self, channel: &str) -> Vec<&str> {
        let mut supported = Vec::new();

        for provider in &self.providers {
            if let Ok(true) = tokio::time::timeout(
                std::time::Duration::from_secs(3),
                async {
                    if !provider.is_available().await {
                        return false;
                    }
                    provider
                        .get_available_versions(
                            channel,
                            std::env::consts::OS,
                            std::env::consts::ARCH,
                        )
                        .await
                        .map(|v| !v.is_empty())
                        .unwrap_or(false)
                }
            )
            .await
            {
                supported.push(provider.name());
            }
        }

        supported
    }

    /// Get the best provider for a specific channel (highest priority that has the channel)
    pub async fn get_best_provider_for_channel(&self, channel: &str) -> Option<&str> {
        let providers = self.get_providers_for_channel(channel).await;
        providers.first().copied()
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
    ) -> Result<Zeroizing<String>> {
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

    /// Get provider names for UI/debugging
    pub fn get_provider_names(&self) -> Vec<&str> {
        self.providers.iter().map(|p| p.name()).collect()
    }

    /// Get number of providers
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }
}

impl Default for PatchApiManager {
    fn default() -> Self {
        Self::new()
    }
}