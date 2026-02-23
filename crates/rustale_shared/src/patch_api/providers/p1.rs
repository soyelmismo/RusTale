//! Provider1 - Backup mirror (Sanasol)
//! 
//! Este provider usa un flujo de dos pasos:
//! 1. GET /api/patches-config → obtiene URL real del CDN
//! 2. Usa esa URL para descargar patches
//!
//! SECURITY: Todos los strings sensibles usan Zeroizing para evitar exposición en RAM

use anyhow::Result;
use async_trait::async_trait;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use zeroize::Zeroizing;
use once_cell::sync::Lazy;
use std::sync::Mutex;
use std::time::Instant;

#[cfg(feature = "security")]
use rustale_security::{RawSecureClient, SecureClient, init_shield, memory::ZeroizeArena};

use crate::patch_api::traits::PatchProvider;
#[cfg(feature = "security")]
use crate::patch_api::utils::{get_pinned_cert_hash, get_private_var};

/// Cache para la URL del CDN obtenida de /api/patches-config
#[cfg(feature = "security")]
struct PatchesConfigCache {
    url: Option<Zeroizing<String>>,
    expires_at: Instant,
}

#[cfg(feature = "security")]
static PATCHES_CONFIG_CACHE: Lazy<Mutex<PatchesConfigCache>> = Lazy::new(|| {
    Mutex::new(PatchesConfigCache {
        url: None,
        expires_at: Instant::now(),
    })
});

/// TTL del caché de patches-config (5 minutos)
const PATCHES_CONFIG_TTL_SECS: u64 = 300;

/// Provider1 - mirror Sanasol con descubrimiento dinámico de CDN
#[cfg(feature = "security")]
pub struct Provider1 {
    client: SecureClient,
    raw_client: RawSecureClient,
}

#[cfg(feature = "security")]
impl Provider1 {
    pub fn new() -> Self {
        init_shield();

        Self {
            client: SecureClient::builder()
                .with_pinning(get_pinned_cert_hash)
                .build(),
            raw_client: RawSecureClient::new(get_pinned_cert_hash),
        }
    }

    /// Obtiene la URL base del CDN desde /api/patches-config
    /// con caché de 5 minutos
    async fn get_patches_base_url(&self) -> Result<Zeroizing<String>> {
        // Verificar caché
        {
            let cache = PATCHES_CONFIG_CACHE.lock().map_err(|_| anyhow::anyhow!("Cache lock error"))?;
            if let Some(ref url) = cache.url {
                if cache.expires_at > Instant::now() {
                    println!("[Provider1] Using cached patches_url");
                    return Ok(url.clone());
                }
            }
        }

        // Obtener del endpoint - TODO en Zeroizing
        let auth_domain = get_private_var("Z_S_A");
        
        // Construir URL sin format! - usar ZeroizeArena
        let mut arena = ZeroizeArena::<512>::new();
        write!(&mut arena, "{}/api/patches-config", &*auth_domain)?;
        
        let config_url = Zeroizing::new(String::from_utf8(arena.as_slice().to_vec())?);
        
        println!("[Provider1] Fetching patches-config");
        
        let response = self.client
            .get(&config_url)
            .header("User-Agent", "Hytale-F2P-Launcher")
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?;
        
        if !response.status().is_success() {
            anyhow::bail!("patches-config returned: {}", response.status());
        }
        
        // El texto de respuesta también es sensible
        let text = Zeroizing::new(response.text().await?);
        
        // Parsear JSON de forma simple
        let patches_url = self.extract_patches_url(&text)?;
        
        println!("[Provider1] Got patches_url from API");
        
        // Guardar en caché
        {
            let mut cache = PATCHES_CONFIG_CACHE.lock().map_err(|_| anyhow::anyhow!("Cache lock error"))?;
            cache.url = Some(patches_url.clone());
            cache.expires_at = Instant::now() + std::time::Duration::from_secs(PATCHES_CONFIG_TTL_SECS);
        }
        
        Ok(patches_url)
    }

    /// Extrae patches_url del JSON de forma segura sin serde
    fn extract_patches_url(&self, json: &str) -> Result<Zeroizing<String>> {
        // Buscar "patches_url":"..."
        if let Some(start) = json.find("\"patches_url\"") {
            if let Some(colon) = json[start..].find(':') {
                let after_colon = &json[start + colon + 1..];
                // Buscar el valor string
                if let Some(open_quote) = after_colon.find('"') {
                    let rest = &after_colon[open_quote + 1..];
                    if let Some(close_quote) = rest.find('"') {
                        let url = &rest[..close_quote];
                        // Remover trailing slash si existe
                        let clean_url = url.trim_end_matches('/');
                        return Ok(Zeroizing::new(clean_url.to_string()));
                    }
                }
            }
        }
        anyhow::bail!("Failed to parse patches_url from JSON");
    }

    async fn check_file_exists_secure_with_mode(&self, url_str: &str, is_patch: bool) -> bool {
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

        let mut host_arena = ZeroizeArena::<256>::new();
        if host_arena.write_all(host_str.as_bytes()).is_err() {
            return false;
        }

        let mut path_arena = ZeroizeArena::<512>::new();
        if path_arena.write_all(path_str.as_bytes()).is_err() {
            return false;
        }

        // Headers mínimos para CDN público
        let ua = Zeroizing::new("Hytale-F2P-Launcher".to_string());

        let raw_client = self.raw_client.clone();

        tokio::task::spawn_blocking(move || {
            let host_ref = std::str::from_utf8(host_arena.as_slice()).unwrap();
            let path_ref = path_arena.as_slice();
            
            let headers = [
                ("User-Agent", ua.as_str()),
            ];

            raw_client
                .head(host_ref, port, path_ref, &headers, !is_patch)
                .unwrap_or(false)
        })
        .await
        .unwrap_or(false)
    }

    fn build_patch_url(
        &self,
        base_url: &str,
        architecture: &str,
        operating_system: &str,
        channel: &str,
        from_version: i32,
        to_version: i32,
    ) -> Zeroizing<String> {
        let arch = match architecture {
            "x86_64" => "amd64",
            "aarch64" => "arm64",
            _ => architecture,
        };

        let os = match operating_system {
            "darwin" => "mac",
            _ => operating_system,
        };

        // Formato: {base_url}/{os}/{arch}/{channel}/{from}_to_{to}.pwr
        let mut arena = ZeroizeArena::<512>::new();
        write!(
            &mut arena,
            "{}/patches/{}/{}/{}/{}_to_{}.pwr",
            base_url, os, arch, channel, from_version, to_version
        ).unwrap();
        
        let bytes = arena.as_slice();
        let mut exact_vec = Vec::with_capacity(bytes.len());
        exact_vec.extend_from_slice(bytes);
        Zeroizing::new(String::from_utf8(exact_vec).unwrap())
    }

    async fn check_version_exists(
        &self,
        base_url: &str,
        start_version: i32,
        end_version: i32,
        architecture: &str,
        operating_system: &str,
        channel: &str,
    ) -> bool {
        let url = self.build_patch_url(base_url, architecture, operating_system, channel, start_version, end_version);
        self.check_file_exists_secure_with_mode(&url, true).await
    }
}

#[cfg(feature = "security")]
#[async_trait]
impl PatchProvider for Provider1 {
    fn name(&self) -> &str {
        "S"
    }

    fn priority(&self) -> i32 {
        90
    }

    fn is_cloudflare(&self) -> bool {
        true
    }

    fn base_url(&self) -> Option<zeroize::Zeroizing<String>> {
        Some(get_private_var("Z_S_A").into_zeroizing())
    }

    async fn is_available(&self) -> bool {
        self.get_patches_base_url().await.is_ok()
    }

    async fn get_latest_version(&self, channel: &str, os: &str, arch: &str) -> Result<i32> {
        let base_url = self.get_patches_base_url().await?;

        let mut last_found = 0;
        let mut next_check = 1;
        let mut step = 2;

        while next_check <= 100 {
            let exists = self
                .check_version_exists(&base_url, 0, next_check, arch, os, channel)
                .await;

            if exists {
                last_found = next_check;
                next_check += step;
                step += 1;
            } else {
                break;
            }
        }

        if last_found == 0 {
            anyhow::bail!("Provider S unreachable or no versions found");
        }

        let mut low = last_found;
        let mut high = next_check - 1;
        let mut result = last_found;

        while low <= high {
            let mid = (low + high) / 2;
            if mid <= result {
                low = mid + 1;
                continue;
            }

            let exists = self.check_version_exists(&base_url, 0, mid, arch, os, channel).await;

            if exists {
                result = mid;
                low = mid + 1;
            } else {
                high = mid - 1;
            }
        }

        Ok(result)
    }

    async fn get_available_versions(
        &self,
        channel: &str,
        os: &str,
        arch: &str,
    ) -> Result<Vec<i32>> {
        let latest = self.get_latest_version(channel, os, arch).await?;

        let mut versions = Vec::new();
        let mut milestones = vec![1, 3, 6, 10];

        if latest > 10 {
            let step = (latest / 10).max(5);
            let mut current = 10 + step;
            while current < latest {
                milestones.push(current);
                current += step;
            }
        }

        let base_url = self.get_patches_base_url().await?;

        for &v in &milestones {
            if v <= latest && self.check_version_exists(&base_url, v - 1, v, arch, os, channel).await {
                versions.push(v);
            }
        }

        if latest > 0
            && self
                .check_version_exists(&base_url, latest - 1, latest, arch, os, channel)
                .await
        {
            versions.push(latest);
        }

        versions.sort();
        versions.dedup();

        Ok(versions)
    }

    async fn get_patch_url(
        &self,
        channel: &str,
        os: &str,
        arch: &str,
        from_version: i32,
        to_version: i32,
    ) -> Result<Zeroizing<String>> {
        let base_url = self.get_patches_base_url().await?;
        let url = self.build_patch_url(&base_url, arch, os, channel, from_version, to_version);
        
        if self.check_file_exists_secure_with_mode(&url, true).await {
            Ok(url)
        } else {
            anyhow::bail!("Patch check failed on Provider S")
        }
    }

    async fn has_complete_version(
        &self,
        channel: &str,
        os: &str,
        arch: &str,
        version: i32,
    ) -> Result<bool> {
        let base_url = self.get_patches_base_url().await?;
        let exists = self
            .check_version_exists(&base_url, 0, version, arch, os, channel)
            .await;
        Ok(exists)
    }

    async fn get_complete_url(
        &self,
        channel: &str,
        os: &str,
        arch: &str,
        version: i32,
    ) -> Result<Zeroizing<String>> {
        let base_url = self.get_patches_base_url().await?;
        let url = self.build_patch_url(&base_url, arch, os, channel, 0, version);
        
        if self.check_file_exists_secure_with_mode(&url, false).await {
            Ok(url)
        } else {
            anyhow::bail!("Complete version check failed on mirror S")
        }
    }

    async fn download_patch_secure(
        &self,
        channel: &str,
        os: &str,
        arch: &str,
        from_version: i32,
        to_version: i32,
        dest_path: &std::path::Path,
        cancel_token: Arc<AtomicBool>,
        progress_callback: Box<dyn Fn(f64, u64, u64) + Send + Sync>,
    ) -> Result<()> {
        // 1. Obtener URL base del CDN
        let patches_url = self.get_patches_base_url().await?;
        
        // 2. Extraer host de la URL (en ZeroizeArena)
        let host = if patches_url.starts_with("https://") {
            &patches_url[8..]
        } else if patches_url.starts_with("http://") {
            &patches_url[7..]
        } else {
            &*patches_url
        };

        // 3. Construir path en ZeroizeArena
        let mut path_arena = ZeroizeArena::<512>::new();
        
        let arch_str = match arch {
            "x86_64" => "amd64",
            "aarch64" => "arm64",
            _ => arch,
        };
        let os_str = match os {
            "darwin" => "mac",
            _ => os,
        };

        write!(
            path_arena,
            "/patches/{}/{}/{}/{}_to_{}.pwr",
            os_str, arch_str, channel, from_version, to_version
        )?;

        // 4. Preparar descarga con host en ZeroizeArena
        let mut host_arena = ZeroizeArena::<256>::new();
        host_arena.write_all(host.as_bytes()).unwrap();

        let ua = Zeroizing::new("Hytale-F2P-Launcher".to_string());

        let raw_client = self.raw_client.clone();
        let dest_path_clone = dest_path.to_path_buf();
        
        tokio::task::spawn_blocking(move || {
            let host_ref = std::str::from_utf8(host_arena.as_slice()).unwrap();
            let headers = [
                ("User-Agent", ua.as_str()),
            ];

            raw_client.get_to_file(
                host_ref,
                443,
                path_arena.as_slice(),
                &headers,
                &dest_path_clone,
                cancel_token,
                progress_callback
            )
        }).await??;

        Ok(())
    }
}

impl Clone for Provider1 {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            raw_client: self.raw_client.clone(),
        }
    }
}

impl Default for Provider1 {
    fn default() -> Self {
        Self::new()
    }
}