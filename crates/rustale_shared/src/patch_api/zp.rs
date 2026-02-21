use anyhow::Result;
use async_trait::async_trait;
use zeroize::Zeroize;

#[cfg(feature = "security")]
use rustale_security::{RawSecureClient, SecureClient, init_shield};

use crate::patch_api::traits::PatchProvider;
#[cfg(feature = "security")]
use crate::patch_api::utils::{get_pinned_cert_hash, get_private_var};

/// Z API provider (private mirror API)
///
/// Este proveedor utiliza el sistema de seguridad RusTale Security Suite
/// para proteger las credenciales y la integridad de las peticiones.
///
/// Características de seguridad activas:
/// 1. Ofuscación de strings Procedural (sec_str! v2).
/// 2. Micro-cliente HTTP/TLS Blindado (RawSecureClient) para pings críticos.
/// 3. Zeroize Arena para construcción de headers volátiles.
/// 4. Honeypots y Degradación Retardada ante Tampering.
#[cfg(feature = "security")]
pub struct ZProvider {
    client: SecureClient,
    raw_client: RawSecureClient,
}

#[cfg(feature = "security")]
impl ZProvider {
    pub fn new() -> Self {
        // Inicializar el sistema de defensa activa (Watchdog)
        init_shield();

        Self {
            // Cliente estándar para descargas grandes (reqwest)
            client: SecureClient::builder()
                .with_pinning(get_pinned_cert_hash)
                .build(),
            // Micro-cliente para peticiones HEAD de metadatos (rustls crudo)
            raw_client: RawSecureClient::new(get_pinned_cert_hash),
        }
    }

    /// Verifica si un archivo existe con modo específico (availability vs patch check)
    #[cfg(feature = "security")]
    async fn check_file_exists_secure_with_mode(&self, url_str: &str, is_patch: bool) -> bool {
        // Parse manual
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

        let mut host = host_str.to_string();
        let mut path = path_str.to_string();

        // Extraemos las credenciales como SafeStrings.
        // No usamos .to_string() para evitar dejar copias en texto plano en el heap.
        let v_header = get_private_var("Z_B");
        let v_val = get_private_var("Z_C");
        let b_header = get_private_var("Z_E");
        let b_val = get_private_var("Z_D");
        let ua_header = get_private_var("Z_G");
        let ua_val = get_private_var("Z_F");

        // Crash si alguna variable crítica no está configurada
        if v_header.is_empty()
            || v_val.is_empty()
            || b_header.is_empty()
            || b_val.is_empty()
            || ua_header.is_empty()
            || ua_val.is_empty()
        {
            panic!("[ZProvider] Critical environment variables missing. Check your .env file.");
        }

        // Ejecutamos la petición HEAD en un hilo de bloqueo para no colgar el runtime async.
        // Movemos los SafeStrings al closure; se limpiarán con zeroize al terminar el bloque.
        let raw_client = self.raw_client.clone();

        tokio::task::spawn_blocking(move || {
            // Reconstruimos el array de referencias dentro del hilo.
            // Esto es seguro porque movemos la propiedad de los SafeStrings al hilo.
            let headers = [
                (&*v_header, &*v_val),
                (&*b_header, &*b_val),
                (&*ua_header, &*ua_val),
            ];

            // DEBUG: Mostrar headers que se envían
            //println!("[DEBUG] Sending headers:");
            //for (k, v) in &headers {
            //    println!("[DEBUG]   {}: {}", k, v);
            //}

            let success = raw_client
                .head(&host, port, &path, &headers, !is_patch) // patches: solo 200, availability: 200 y 301
                .unwrap_or(false);

            // WIPE HEAP DATA SECURELY
            use zeroize::Zeroize;
            host.zeroize();
            path.zeroize();

            success
        })
        .await
        .unwrap_or(false)
    }

    /// Try to guess the patch URL for the given parameters
    fn guess_patch_url_no_auth(
        &self,
        architecture: &str,
        operating_system: &str,
        channel: &str,
        from_version: i32,
        to_version: i32,
    ) -> String {
        let arch = match architecture {
            "x86_64" => "amd64",
            "aarch64" => "arm64",
            _ => architecture,
        };

        let os = match operating_system {
            "darwin" => "mac",
            _ => operating_system,
        };

        let result = format!(
            "{}/patches/{}/{}/{}/{}/{}.pwr",
            &*get_private_var("Z_A"),
            os,   // linux
            arch, // amd64
            channel,
            from_version,
            to_version
        );

        // DEBUG: Verificar URL generada
        //println!("[DEBUG] Generated URL: {}", result);
        result
    }

    async fn check_version_exists(
        &self,
        start_version: i32,
        end_version: i32,
        architecture: &str,
        operating_system: &str,
        channel: &str,
    ) -> bool {
        let mut url = self.guess_patch_url_no_auth(
            architecture,
            operating_system,
            channel,
            start_version,
            end_version,
        );
        let res = self.check_file_exists_secure_with_mode(&url, true).await;
        url.zeroize(); // WIPE FROM RAM
        res
    }
}

#[cfg(feature = "security")]
#[async_trait]
impl PatchProvider for ZProvider {
    fn name(&self) -> &str {
        "Z"
    }

    fn priority(&self) -> i32 {
        100 // Mayor prioridad ahora que es Blindada
    }

    async fn is_available(&self) -> bool {
        // Probar con un patch específico que sabemos que funciona
        let mut test_url = format!("{}/", &*get_private_var("Z_A"));
        let res = self
            .check_file_exists_secure_with_mode(&test_url, false)
            .await;
        test_url.zeroize(); // WIPE FROM RAM
        res
    }

    async fn get_latest_version(&self, channel: &str, os: &str, arch: &str) -> Result<i32> {
        //println!("[ZProvider] Starting exponential search for {}/{}/{}", os, arch, channel);

        // Fase 1: Búsqueda exponencial para encontrar el rango
        let mut last_found = 0;
        let mut next_check = 1;
        let mut step = 2;

        while next_check <= 100 {
            // Límite más razonable
            //println!("[ZProvider] Exponential check: {}", next_check);

            // Buscar patch desde 0 hasta next_check (0->N)
            let exists = self
                .check_version_exists(0, next_check, arch, os, channel)
                .await;
            //println!("[ZProvider] Version 0->{} exists: {}", next_check, exists);

            if exists {
                last_found = next_check;
                next_check += step;
                step += 1; // Incremento creciente: 2, 3, 4, 5...
            } else {
                // Encontramos el límite superior, ahora búsqueda binaria
                break;
            }
        }

        if last_found == 0 {
            anyhow::bail!("Z Server is unreachable or invalid credentials");
        }

        // Fase 2: Búsqueda binaria entre last_found y next_check-1
        let mut low = last_found;
        let mut high = next_check - 1;
        let mut result = last_found;

        while low <= high {
            let mid = (low + high) / 2;
            if mid <= result {
                low = mid + 1;
                continue;
            }

            //println!("[ZProvider] Binary search check: {}", mid);
            let exists = self.check_version_exists(0, mid, arch, os, channel).await;
            //println!("[ZProvider] Version 0->{} exists: {}", mid, exists);

            if exists {
                result = mid;
                low = mid + 1;
            } else {
                high = mid - 1;
            }
        }

        //println!("[ZProvider] Latest version found: {}", result);
        Ok(result)
    }

    async fn get_available_versions(
        &self,
        channel: &str,
        os: &str,
        arch: &str,
    ) -> Result<Vec<i32>> {
        let latest = self.get_latest_version(channel, os, arch).await?;

        // Optimized: Instead of checking every patch, use dynamic milestones
        let mut versions = Vec::new();

        // Generate dynamic milestones based on the latest version
        let mut milestones = Vec::new();

        // Always include early versions
        milestones.extend_from_slice(&[1, 3, 6, 10]);

        // Add intermediate milestones for larger versions
        if latest > 10 {
            let step = (latest / 10).max(5); // Step of 5 or 1/10 of latest, whichever is larger
            let mut current = 10 + step;
            while current < latest {
                milestones.push(current);
                current += step;
            }
        }

        // Check all milestones
        for &v in &milestones {
            if v <= latest && self.check_version_exists(v - 1, v, arch, os, channel).await {
                versions.push(v);
            }
        }

        // Always include the latest version if we can reach it
        if latest > 0
            && self
                .check_version_exists(latest - 1, latest, arch, os, channel)
                .await
        {
            versions.push(latest);
        }

        // Sort and deduplicate
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
    ) -> Result<zeroize::Zeroizing<String>> {
        let mut url = self.guess_patch_url_no_auth(arch, os, channel, from_version, to_version);
        if self.check_file_exists_secure_with_mode(&url, true).await {
            Ok(zeroize::Zeroizing::new(url))
        } else {
            url.zeroize(); // WIPE FROM RAM
            anyhow::bail!("Patch check failed on Z") // REMOVED LEAKING URL
        }
    }

    async fn has_complete_version(
        &self,
        channel: &str,
        os: &str,
        arch: &str,
        version: i32,
    ) -> Result<bool> {
        // Check if complete patch 0->version exists
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
    ) -> Result<zeroize::Zeroizing<String>> {
        let mut url = self.guess_patch_url_no_auth(arch, os, channel, 0, version);
        if self.check_file_exists_secure_with_mode(&url, false).await {
            Ok(zeroize::Zeroizing::new(url))
        } else {
            url.zeroize(); // WIPE FROM RAM
            anyhow::bail!("Complete version check failed on Z") // REMOVED LEAKING URL
        }
    }
}

// Implementación de Clone manual necesaria para spawn_blocking
impl Clone for ZProvider {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            raw_client: self.raw_client.clone(),
        }
    }
}

impl Default for ZProvider {
    fn default() -> Self {
        Self::new()
    }
}
