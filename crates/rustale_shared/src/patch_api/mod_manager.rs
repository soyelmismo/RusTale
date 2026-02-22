use anyhow::Result;
use zeroize::Zeroizing;
use std::sync::Arc;

#[cfg(feature = "security")]
use rustale_security::{RawSecureClient, SecureClient, SafeString, init_shield};

use crate::patch_api::traits::PatchProvider;
#[cfg(feature = "security")]
use crate::patch_api::utils::{get_pinned_cert_hash, get_private_var};
#[cfg(feature = "security")]
use crate::patch_api::providers::{Provider0, Provider1, Provider2, Provider3};
#[cfg(feature = "security")]
use crate::network::SecureHeaders;

// ============================================================================
// MIRROR CONFIGURATION TYPES
// ============================================================================

/// Mirror configuration structure with secure headers
#[derive(Debug)]
pub struct MirrorConfig {
    pub name: String,
    pub base_url: Zeroizing<String>,
    pub priority: i32,
    pub is_cloudflare: bool,
    /// Secure headers that automatically zeroize when dropped
    #[cfg(feature = "security")]
    pub auth_headers: Option<SecureHeaders>,
    #[cfg(not(feature = "security"))]
    pub auth_headers: Option<Vec<(String, String)>>,
}

/// Estadísticas de mirrors para UI/monitoring
#[derive(Debug, Clone)]
pub struct MirrorStats {
    pub name: String,
    pub priority: i32,
    pub is_cloudflare: bool,
    pub is_current: bool,
}

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
// MIRROR MANAGER
// ============================================================================

/// Mirror Manager - Gestión centralizada de mirrors
/// 
/// Este módulo implementa la lógica de fallback y rotación de mirrors
/// basada en el sistema Hytale-F2P JavaScript pero adaptada a Rust.
pub struct MirrorManager {
    #[cfg(feature = "security")]
    client: SecureClient,
    #[cfg(feature = "security")]
    raw_client: RawSecureClient,
    mirrors: Vec<MirrorConfig>,
    current_index: usize,
}

impl MirrorManager {
    #[cfg(feature = "security")]
    pub fn new() -> Self {
        init_shield();

        // Helper to convert SafeString to Zeroizing<String> without intermediate copy
        fn to_zeroizing(s: SafeString) -> Zeroizing<String> {
            s.into_zeroizing()
        }

        // Helper to build SecureHeaders from SafeStrings
        fn build_secure_headers(k1: SafeString, v1: SafeString, k2: SafeString, v2: SafeString, k3: SafeString, v3: SafeString) -> SecureHeaders {
            let mut headers = SecureHeaders::new();
            headers.push(k1, v1);
            headers.push(k2, v2);
            headers.push(k3, v3);
            headers
        }

        let mirrors = vec![
            // Provider0 - Primary mirror - highest priority
            MirrorConfig {
                name: "E".to_string(),
                base_url: to_zeroizing(get_private_var("Z_E_A")),
                priority: 100,
                is_cloudflare: true,
                auth_headers: Some(build_secure_headers(
                    get_private_var("Z_E_B"), get_private_var("Z_E_C"),
                    get_private_var("Z_E_E"), get_private_var("Z_E_D"),
                    get_private_var("Z_E_G"), get_private_var("Z_E_F"),
                )),
            },
            // Provider1 - Backup mirror
            MirrorConfig {
                name: "S".to_string(),
                base_url: to_zeroizing(get_private_var("Z_S_A")),
                priority: 90,
                is_cloudflare: true,
                auth_headers: Some(build_secure_headers(
                    get_private_var("Z_S_B"), get_private_var("Z_S_C"),
                    get_private_var("Z_S_E"), get_private_var("Z_S_D"),
                    get_private_var("Z_S_G"), get_private_var("Z_S_F"),
                )),
            },
            // Provider2 - Non-Cloudflare mirrors
            MirrorConfig {
                name: "H1".to_string(),
                base_url: to_zeroizing(get_private_var("Z_H_A")),
                priority: 80,
                is_cloudflare: false,
                auth_headers: Some(build_secure_headers(
                    get_private_var("Z_H_C"), get_private_var("Z_H_D"),
                    get_private_var("Z_H_E"), get_private_var("Z_H_F"),
                    get_private_var("Z_H_G"), get_private_var("Z_H_H"),
                )),
            },
            MirrorConfig {
                name: "H2".to_string(),
                base_url: to_zeroizing(get_private_var("Z_H_B")),
                priority: 75,
                is_cloudflare: false,
                auth_headers: Some(build_secure_headers(
                    get_private_var("Z_H_C"), get_private_var("Z_H_D"),
                    get_private_var("Z_H_E"), get_private_var("Z_H_F"),
                    get_private_var("Z_H_G"), get_private_var("Z_H_H"),
                )),
            },
            // Provider3 - Hardcoded fallback - public
            MirrorConfig {
                name: "V".to_string(),
                base_url: to_zeroizing(get_private_var("Z_V_A")),
                priority: 50,
                is_cloudflare: false,
                auth_headers: None, // Público, no requiere autenticación
            },
        ];

        Self {
            client: SecureClient::builder()
                .with_pinning(get_pinned_cert_hash)
                .build(),
            raw_client: RawSecureClient::new(get_pinned_cert_hash),
            mirrors,
            current_index: 0,
        }
    }

    /// Obtiene el mirror actual basado en disponibilidad y prioridad
    pub async fn get_best_mirror(&mut self) -> Option<&MirrorConfig> {
        // Primero intentar con el mirror actual
        if self.current_index < self.mirrors.len() {
            if self.check_mirror_availability(&self.mirrors[self.current_index]).await {
                return Some(&self.mirrors[self.current_index]);
            }
        }

        // Si no funciona, buscar el mejor disponible
        for (i, mirror) in self.mirrors.iter().enumerate() {
            if self.check_mirror_availability(mirror).await {
                self.current_index = i;
                return Some(mirror);
            }
        }

        None
    }

    /// Verifica si un mirror está disponible
    #[cfg(feature = "security")]
    async fn check_mirror_availability(&self, mirror: &MirrorConfig) -> bool {
        use std::io::Write;
        let mut arena = rustale_security::memory::ZeroizeArena::<512>::new();
        write!(&mut arena, "{}/manifest.json", &*mirror.base_url).unwrap();
        
        let url_str = std::str::from_utf8(arena.as_slice()).unwrap();
        
        let without_scheme = if url_str.starts_with("https://") {
            &url_str[8..]
        } else if url_str.starts_with("http://") {
            &url_str[7..]
        } else {
            url_str
        };

        let slash_idx = without_scheme.find('/').unwrap_or(without_scheme.len());
        let host_port = &without_scheme[..slash_idx];
        let path_str = if slash_idx < without_scheme.len() {
            &without_scheme[slash_idx..]
        } else {
            "/"
        };

        let (host_str, port) = if let Some(colon_idx) = host_port.find(':') {
            let port_str = &host_port[colon_idx + 1..];
            let p = port_str.parse::<u16>().unwrap_or(443);
            (&host_port[..colon_idx], p)
        } else {
            (host_port, 443)
        };

        // STACK ALLOCATION
        let mut host_arena = rustale_security::memory::ZeroizeArena::<256>::new();
        host_arena.write_all(host_str.as_bytes()).unwrap();

        let mut path_arena = rustale_security::memory::ZeroizeArena::<512>::new();
        path_arena.write_all(path_str.as_bytes()).unwrap();

        let raw_client = self.raw_client.clone();

        let headers_owned: Vec<(zeroize::Zeroizing<String>, zeroize::Zeroizing<String>)> = if let Some(ref secure_headers) = mirror.auth_headers {
            secure_headers.as_ref_pairs()
                .into_iter()
                .map(|(k, v)| (zeroize::Zeroizing::new(k.to_string()), zeroize::Zeroizing::new(v.to_string())))
                .collect()
        } else {
            vec![
                (zeroize::Zeroizing::new("User-Agent".to_string()), zeroize::Zeroizing::new("Hytale-F2P-Launcher-Rust".to_string())),
                (zeroize::Zeroizing::new("Accept".to_string()), zeroize::Zeroizing::new("application/json".to_string())),
                (zeroize::Zeroizing::new("Connection".to_string()), zeroize::Zeroizing::new("keep-alive".to_string())),
            ]
        };

        tokio::task::spawn_blocking(move || {
            let host_ref = std::str::from_utf8(host_arena.as_slice()).unwrap();
            let path_ref = path_arena.as_slice();

            let headers_refs: Vec<(&str, &str)> = headers_owned.iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();

            raw_client
                .head(host_ref, port, path_ref, &headers_refs, false)
                .unwrap_or(false)
        })
        .await
        .unwrap_or(false)
    }

    /// Obtiene todos los mirrors ordenados por prioridad
    pub fn get_all_mirrors(&self) -> Vec<&MirrorConfig> {
        let mut mirrors = self.mirrors.iter().collect::<Vec<_>>();
        mirrors.sort_by(|a, b| b.priority.cmp(&a.priority));
        mirrors
    }

    /// Forzar el uso de un mirror específico por nombre
    pub fn force_mirror(&mut self, name: &str) -> bool {
        if let Some((index, _)) = self.mirrors.iter().enumerate().find(|(_, m)| m.name == name) {
            self.current_index = index;
            true
        } else {
            false
        }
    }

    /// Obtiene estadísticas de los mirrors
    pub fn get_mirror_stats(&self) -> Vec<MirrorStats> {
        self.mirrors.iter().map(|m| MirrorStats {
            name: m.name.clone(),
            priority: m.priority,
            is_cloudflare: m.is_cloudflare,
            is_current: self.mirrors.get(self.current_index).map_or(false, |current| current.name == m.name),
        }).collect()
    }
}

#[cfg(feature = "security")]
impl Clone for MirrorManager {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            raw_client: self.raw_client.clone(),
            mirrors: self.mirrors.iter().map(|m| MirrorConfig {
                name: m.name.clone(),
                base_url: m.base_url.clone(),
                priority: m.priority,
                is_cloudflare: m.is_cloudflare,
                auth_headers: m.auth_headers.as_ref().map(|h| {
                    // Clone SecureHeaders - this creates new SafeStrings
                    let mut new_headers = SecureHeaders::new();
                    for (k, v) in &h.as_ref_pairs() {
                        new_headers.push(SafeString::new(k.to_string()), SafeString::new(v.to_string()));
                    }
                    new_headers
                }),
            }).collect(),
            current_index: self.current_index,
        }
    }
}

impl Default for MirrorManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// PATCH API MANAGER
// ============================================================================

/// Generic patch API manager that handles multiple providers
/// 
/// Providers are ordered by priority:
/// 1. Provider0 (100) - Primary private mirror
/// 2. Provider1 (90) - Backup mirror
/// 3. Provider2 (80) - Non-Cloudflare mirrors
/// 4. Provider3 (50) - Hardcoded fallback
pub struct PatchApiManager {
    providers: Vec<Arc<dyn PatchProvider>>,
    #[cfg(feature = "security")]
    mirror_manager: MirrorManager,
}

impl PatchApiManager {
    pub fn new() -> Self {
        let mut providers: Vec<Arc<dyn PatchProvider>> = Vec::new();

        // Add all available providers in order of preference (highest priority first)
        #[cfg(feature = "security")]
        {
            providers.push(Arc::new(Provider0::new()));
            providers.push(Arc::new(Provider1::new()));
            providers.push(Arc::new(Provider2::new()));
            providers.push(Arc::new(Provider3::new()));
        }

        Self {
            providers,
            #[cfg(feature = "security")]
            mirror_manager: MirrorManager::new(),
        }
    }

    /// Get the mirror manager instance
    #[cfg(feature = "security")]
    pub fn mirror_manager(&self) -> &MirrorManager {
        &self.mirror_manager
    }

    /// Get mutable mirror manager instance
    #[cfg(feature = "security")]
    pub fn mirror_manager_mut(&mut self) -> &mut MirrorManager {
        &mut self.mirror_manager
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