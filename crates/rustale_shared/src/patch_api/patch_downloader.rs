use anyhow::Result;
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};

use crate::ProgressCallback;
use crate::patch_api::integrity_checker::IntegrityChecker;
use crate::patch_api::utils::*;

#[cfg(feature = "security")]
use crate::patch_api::providers::get_all_providers;

/// Gestor de descargas de parches del juego
#[derive(Clone)]
pub struct PatchDownloader {}

impl PatchDownloader {
    pub fn new() -> Self {
        Self {}
    }

    /// Descarga un parche para el rango de versiones especificado con fallback automático
    pub async fn download_patch(
        &self,
        channel: &str,
        from_version: i32,
        to_version: i32,
        progress_callback: ProgressCallback,
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> Result<PathBuf> {
        let cache_dir = crate::config::get_cache_dir("patches").await;
        tokio::fs::create_dir_all(&cache_dir).await?;

        // Intentar primero el parche directo (from -> to)
        // Si no existe o falla, intentar el parche completo (0 -> to)
        let attempts = if from_version > 0 {
            vec![(from_version, "direct"), (0, "complete")]
        } else {
            vec![(0, "direct/complete")]
        };

        let mut last_error = None;

        for (actual_from, strategy) in attempts {
            let filename = format!(
                "{}~{}-{}-{}.pwr",
                actual_from,
                to_version,
                std::env::consts::OS,
                get_arch_name()
            );
            let patch_path = cache_dir.join(&filename);

            if patch_path.exists() {
                if let Ok(meta) = std::fs::metadata(&patch_path) {
                    if meta.len() > 0 {
                        // Ya existe y no está vacío, verificar integridad
                        if self.verify_integrity(&patch_path, progress_callback.clone(), cancel_token.clone()).await.is_ok() {
                            return Ok(patch_path);
                        }
                    }
                }
                let _ = tokio::fs::remove_file(&patch_path).await;
            }

            progress_callback(
                "patch".to_string(),
                0.0,
                format!("Downloading {} ({}→{})...", strategy, actual_from, to_version),
                0, 0, None, Some(0),
            );

            #[cfg(feature = "security")]
            {
                let providers = get_all_providers();
                let cancel_token_ref = cancel_token.clone().unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
                let mut download_success = false;

                for provider in providers {
                    if !provider.is_available().await { continue; }

                    let progress_cb = Box::new({
                        let p_cb = progress_callback.clone();
                        let strat = strategy.to_string();
                        move |pct: f64, total: u64, downloaded: u64| {
                            p_cb("patch".to_string(), pct, format!("{}: {:.1} MB", strat, downloaded as f64 / 1_048_576.0), total, downloaded, None, Some(0));
                        }
                    });

                    if provider.download_patch_secure(channel, std::env::consts::OS, get_arch_name(), actual_from, to_version, &patch_path, cancel_token_ref.clone(), progress_cb).await.is_ok() {
                        download_success = true;
                        break;
                    }
                }

                if download_success {
                    if self.verify_integrity(&patch_path, progress_callback.clone(), cancel_token.clone()).await.is_ok() {
                        return Ok(patch_path);
                    }
                }
            }
            
            last_error = Some(anyhow::anyhow!("Failed to download patch using strategy {}", strategy));
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("All download attempts failed")))
    }

    async fn verify_integrity(&self, path: &PathBuf, progress_callback: ProgressCallback, cancel_token: Option<Arc<AtomicBool>>) -> Result<()> {
        let checker = IntegrityChecker::new();
        let cb = move |p: f64, _m: &str| {
            progress_callback("verify".to_string(), p * 100.0, "Verifying...".to_string(), 0, 0, None, Some(1));
        };

        if checker.verify_download_integrity(path, None, None, Some(cb), cancel_token).await?.is_valid() {
            Ok(())
        } else {
            let _ = tokio::fs::remove_file(path).await;
            anyhow::bail!("Integrity check failed")
        }
    }

    /// Descarga una versión completa (alias de download_patch con from=0)
    pub async fn download_complete_version(
        &self,
        channel: &str,
        target_version: i32,
        progress_callback: ProgressCallback,
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> Result<PathBuf> {
        self.download_patch(channel, 0, target_version, progress_callback, cancel_token).await
    }

    /// Comprueba si el parche ya está en caché
    pub async fn is_patch_cached(&self, from_version: i32, to_version: i32) -> Result<bool> {
        let cache_dir = crate::config::get_cache_dir("patches").await;
        let os = std::env::consts::OS;
        let arch = get_arch_name();

        let paths = [
            cache_dir.join(format!("{}~{}-{}-{}.pwr", from_version, to_version, os, arch)),
            cache_dir.join(format!("0~{}-{}-{}.pwr", to_version, os, arch)),
        ];

        for p in paths {
            if p.exists() {
                if let Ok(meta) = std::fs::metadata(&p) {
                    if meta.len() > 0 { return Ok(true); }
                }
            }
        }
        Ok(false)
    }
}
