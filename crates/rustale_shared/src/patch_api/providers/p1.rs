//! Provider1 - Backup mirror (S)
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

/// Provider1
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
                .build(),
            raw_client: RawSecureClient::new(),
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

        println!("[Provider1] Fetching patches-config");
        
        let header_keys = [
            ("Z_S_B", "Z_S_C"),
            ("Z_S_E", "Z_S_D"),
            ("Z_S_G", "Z_S_F"),
        ];

        let config_path = rustale_security::require_private_var("Z_S_H")?.into_string();
        let text_response = self.client.fetch_json_with_keys("Z_S_A", &config_path, &header_keys).await?;
        
        // El texto de respuesta también es sensible
        let text = Zeroizing::new(text_response);
        
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
        let key = rustale_security::require_private_var("Z_S_I")?.into_string();
        let search_key = format!("\"{}\"", key);
        // Buscar "patches_url":"..."
        if let Some(start) = json.find(&search_key) {
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

    async fn check_file_exists(&self, base_url: &str, path_str: &str, is_patch: bool) -> bool {
        let mut path_arena = ZeroizeArena::<512>::new();
        if path_arena.write_all(path_str.as_bytes()).is_err() {
            return false;
        }

        let raw_client = self.raw_client.clone();
        let base_url_owned = base_url.to_string();

        tokio::task::spawn_blocking(move || {
            let path_ref = path_arena.as_slice();
            
            let headers = [
                ("Z_S_B", "Z_S_C"),
            ];

            raw_client
                .request_secure_head_dynamic(&base_url_owned, path_ref, &headers, !is_patch)
                .unwrap_or(false)
        })
        .await
        .unwrap_or(false)
    }

    // Helper removido en favor de uso directo de arena en llamadas 

    async fn check_version_exists(
        &self,
        base_url: &str,
        start_version: i32,
        end_version: i32,
        architecture: &str,
        operating_system: &str,
        channel: &str,
    ) -> bool {
        let mut path_arena = ZeroizeArena::<512>::new();
        if crate::patch_api::utils::write_patch_path_to_arena(
            &mut path_arena, operating_system, architecture, channel, start_version, end_version, "Z_S_T"
        ).is_err() {
            return false;
        }
        
        let path_str = std::str::from_utf8(path_arena.as_slice()).unwrap_or("");
        self.check_file_exists(base_url, path_str, true).await
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
        let patches_url = self.get_patches_base_url().await?;
        
        let mut path_arena = ZeroizeArena::<512>::new();
        crate::patch_api::utils::write_patch_path_to_arena(
            &mut path_arena, os, arch, channel, from_version, to_version, "Z_S_T"
        ).map_err(|e| anyhow::anyhow!("Failed to format path: {}", e))?;

        let raw_client = self.raw_client.clone();
        let dest_path_clone = dest_path.to_path_buf();
        let patches_url_owned = patches_url.to_string();
        
        tokio::task::spawn_blocking(move || {
            let headers = [
                ("Z_S_B", "Z_S_C"),
            ];

            raw_client.download_secure_file_dynamic(
                &patches_url_owned,
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