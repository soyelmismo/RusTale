//! Provider2 - Non-Cloudflare mirror

use anyhow::Result;
use async_trait::async_trait;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use zeroize::Zeroizing;

#[cfg(feature = "security")]
use rustale_security::{RawSecureClient, memory::ZeroizeArena};

use crate::patch_api::traits::PatchProvider;
#[cfg(feature = "security")]
use crate::patch_api::utils::{get_pinned_cert_hash, get_private_var};

/// Provider2 - mirror
#[cfg(feature = "security")]
pub struct Provider2 {
    raw_client: RawSecureClient,
}

#[cfg(feature = "security")]
impl Provider2 {
    pub fn new() -> Self {
        Self {
            raw_client: RawSecureClient::new(get_pinned_cert_hash),
        }
    }

    async fn check_file_exists_secure_with_mode(&self, url_str: &str, is_patch: bool) -> bool {
        use rustale_security::memory::ZeroizeArena;

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

        // BUGFIX: Utilizamos ZeroizeArena estáticos en lugar de Strings en el Heap.
        // Esto mantiene los datos sensibles atrapados en el Stack.
        let mut host_arena = ZeroizeArena::<256>::new();
        host_arena.write_all(host_str.as_bytes()).unwrap();

        let mut path_arena = ZeroizeArena::<512>::new();
        path_arena.write_all(path_str.as_bytes()).unwrap();

        // Z_H_* variables for Provider2
        let v_header = get_private_var("Z_H_C");
        let v_val = get_private_var("Z_H_D");
        let b_header = get_private_var("Z_H_E");
        let b_val = get_private_var("Z_H_F");
        let ua_header = get_private_var("Z_H_G");
        let ua_val = get_private_var("Z_H_H");

        if v_header.is_empty()
            || v_val.is_empty()
            || b_header.is_empty()
            || b_val.is_empty()
            || ua_header.is_empty()
            || ua_val.is_empty()
        {
            return false;
        }

        let raw_client = self.raw_client.clone();

        // Al mover los Arenas al thread de Tokio, este se adueña de ellos
        // y los zeroiza correctamente al finalizar el closure de manera automática.
        tokio::task::spawn_blocking(move || {
            let host_ref = std::str::from_utf8(host_arena.as_slice()).unwrap();
            let path_ref = path_arena.as_slice();
            
            let headers = [
                (&*v_header, &*v_val),
                (&*b_header, &*b_val),
                (&*ua_header, &*ua_val),
            ];

            raw_client
                .head(host_ref, port, path_ref, &headers, !is_patch)
                .unwrap_or(false)
        })
        .await
        .unwrap_or(false)
    }

    fn guess_patch_url_no_auth(
        &self,
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

        let base = get_private_var("Z_H_A");
        
        // ¡REEMPLAZO CLAVE DE FORMAT!: Usamos ZeroizeArena que NO CREA FRAGMENTOS en Heap
        let mut arena = rustale_security::memory::ZeroizeArena::<512>::new();
        write!(
            &mut arena,
            "{}/patches/{}/{}/{}/{}_to_{}.pwr",
            &*base, os, arch, channel, from_version, to_version
        ).unwrap();
        
        // Conversión exacta sin sobre-asignación de capacidad
        let bytes = arena.as_slice();
        let mut exact_vec = Vec::with_capacity(bytes.len());
        exact_vec.extend_from_slice(bytes);
        Zeroizing::new(String::from_utf8(exact_vec).unwrap())
    }

    async fn check_version_exists(
        &self,
        start_version: i32,
        end_version: i32,
        architecture: &str,
        operating_system: &str,
        channel: &str,
    ) -> bool {
        let arch = match architecture {
            "x86_64" => "amd64",
            "aarch64" => "arm64",
            _ => architecture,
        };

        let os = match operating_system {
            "darwin" => "mac",
            _ => operating_system,
        };

        let base = get_private_var("Z_H_A");
        
        // BUGFIX: Construimos la URL en el stack en lugar de en el Heap.
        let mut arena = ZeroizeArena::<512>::new();
        write!(
            &mut arena,
            "{}/patches/{}/{}/{}/{}_to_{}.pwr",
            &*base, os, arch, channel, start_version, end_version
        ).unwrap();
        
        let url_str = std::str::from_utf8(arena.as_slice()).unwrap();
        self.check_file_exists_secure_with_mode(url_str, true).await
    }
}

#[cfg(feature = "security")]
#[async_trait]
impl PatchProvider for Provider2 {
    fn name(&self) -> &str {
        "H1"
    }

    fn priority(&self) -> i32 {
        80
    }

    fn is_cloudflare(&self) -> bool {
        false
    }

    fn base_url(&self) -> Option<zeroize::Zeroizing<String>> {
        Some(get_private_var("Z_H_A").into_zeroizing())
    }

    async fn is_available(&self) -> bool {
        let base = get_private_var("Z_H_A");
        
        // EVITAR format! - Usamos el Arena seguro del Stack
        let mut arena = rustale_security::memory::ZeroizeArena::<512>::new();
        write!(&mut arena, "{}/manifest.json", &*base).unwrap();
        
        let test_url = Zeroizing::new(String::from_utf8(arena.as_slice().to_vec()).unwrap());
        self.check_file_exists_secure_with_mode(&test_url, false).await
    }

    async fn get_latest_version(&self, channel: &str, os: &str, arch: &str) -> Result<i32> {
        let mut last_found = 0;
        let mut next_check = 1;
        let mut step = 2;

        while next_check <= 100 {
            let exists = self
                .check_version_exists(0, next_check, arch, os, channel)
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
            anyhow::bail!("Provider2 unreachable or invalid credentials");
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

            let exists = self.check_version_exists(0, mid, arch, os, channel).await;

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

        for &v in &milestones {
            if v <= latest && self.check_version_exists(v - 1, v, arch, os, channel).await {
                versions.push(v);
            }
        }

        if latest > 0
            && self
                .check_version_exists(latest - 1, latest, arch, os, channel)
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
        let url = self.guess_patch_url_no_auth(arch, os, channel, from_version, to_version);
        if self.check_file_exists_secure_with_mode(&url, true).await {
            Ok(url)
        } else {
            anyhow::bail!("Patch check failed on Provider2")
        }
    }

    async fn has_complete_version(
        &self,
        channel: &str,
        os: &str,
        arch: &str,
        version: i32,
    ) -> Result<bool> {
        let exists = self
            .check_version_exists(0, version, arch, os, channel)
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
        let url = self.guess_patch_url_no_auth(arch, os, channel, 0, version);
        if self.check_file_exists_secure_with_mode(&url, false).await {
            Ok(url)
        } else {
            anyhow::bail!("Complete version check failed on mirror H1")
        }
    }

    /// Descarga un patch directamente a disco sin crear Strings en el Heap.
    /// Implementación Zero-Trace con variables Z_H_*
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
        // 1. Obtener el dominio base de forma segura (Z_H_A para Provider2)
        let base_domain = get_private_var("Z_H_A");
        
        let host = if base_domain.starts_with("https://") {
            &base_domain[8..]
        } else if base_domain.starts_with("http://") {
            &base_domain[7..]
        } else {
            &*base_domain
        };

        // 2. Armar el Path SIN asignar memoria en el Heap
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

        // 3. Extraer cabeceras seguras (Z_H_* para Provider2)
        let v_header = get_private_var("Z_H_C");
        let v_val = get_private_var("Z_H_D");
        let b_header = get_private_var("Z_H_E");
        let b_val = get_private_var("Z_H_F");
        let ua_val = get_private_var("Z_H_H");

        // BUGFIX: Usamos ZeroizeArena para el host en lugar de Zeroizing<String>
        let mut host_arena = ZeroizeArena::<256>::new();
        host_arena.write_all(host.as_bytes()).unwrap();

        // 4. Ejecutar la descarga bloqueante
        let raw_client = self.raw_client.clone();
        let dest_path_clone = dest_path.to_path_buf();
        
        tokio::task::spawn_blocking(move || {
            let host_ref = std::str::from_utf8(host_arena.as_slice()).unwrap();
            let headers = [
                (v_header.as_str(), v_val.as_str()),
                (b_header.as_str(), b_val.as_str()),
                ("User-Agent", ua_val.as_str()),
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

impl Clone for Provider2 {
    fn clone(&self) -> Self {
        Self {
            raw_client: self.raw_client.clone(),
        }
    }
}

impl Default for Provider2 {
    fn default() -> Self {
        Self::new()
    }
}