use anyhow::Result;
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};

use crate::ProgressCallback;
use crate::patch_api::integrity_checker::IntegrityChecker;
use crate::patch_api::mod_manager::PatchApiManager;
use crate::patch_api::traits::PatchProvider;
use crate::patch_api::utils::*;

#[cfg(feature = "security")]
use crate::patch_api::providers::{Provider0, Provider1, Provider2, Provider3};

/// Downloader for game patches
#[derive(Clone)]
pub struct PatchDownloader {}

impl PatchDownloader {
    pub fn new() -> Self {
        Self {}
    }

    /// Try different patch strategies in order: direct -> sequential -> complete
    async fn try_patch_strategies(
        &self,
        channel: &str,
        from_version: i32,
        to_version: i32,
    ) -> Result<(zeroize::Zeroizing<String>, i32, String)> {
        // Strategy 1: Direct patch (from -> to)
        println!(
            "[Downloader] Strategy 1: Trying direct patch {}->{}",
            from_version, to_version
        );
        match PatchApiManager::get_patch_url_static(
            channel,
            std::env::consts::OS,
            get_arch_name(),
            from_version,
            to_version,
        )
        .await
        {
            Ok(url) => {
                println!(
                    "[Downloader] ✓ Direct patch {}->{} available",
                    from_version, to_version
                );
                return Ok((url, from_version, "direct".to_string()));
            }
            Err(_) => {
                println!(
                    "[Downloader] ✗ Direct patch {}->{} failed",
                    from_version, to_version
                );
            }
        }

        // Strategy 2: Sequential patches (find the shortest path)
        if from_version > 0 && from_version < to_version {
            println!(
                "[Downloader] Strategy 2: Finding sequential path from {} to {}",
                from_version, to_version
            );

            // Try to find the shortest sequential path
            let mut path = Vec::new();
            let mut current = from_version;

            while current < to_version {
                let next = current + 1;

                // Check if patch current -> next exists
                if PatchApiManager::get_patch_url_static(
                    channel,
                    std::env::consts::OS,
                    get_arch_name(),
                    current,
                    next,
                )
                .await
                .is_ok()
                {
                    path.push((current, next));
                    current = next;
                } else {
                    // Try jumping further
                    let mut found = false;
                    for jump in 2..=5 {
                        // Try jumps of 2, 3, 4, 5
                        let jump_target = current + jump;
                        if jump_target <= to_version
                            && PatchApiManager::get_patch_url_static(
                                channel,
                                std::env::consts::OS,
                                get_arch_name(),
                                current,
                                jump_target,
                            )
                            .await
                            .is_ok()
                        {
                            path.push((current, jump_target));
                            current = jump_target;
                            found = true;
                            break;
                        }
                    }

                    if !found {
                        println!("[Downloader] ✗ No sequential path found from {}", current);
                        break;
                    }
                }
            }

            if current == to_version && !path.is_empty() {
                println!("[Downloader] ✓ Sequential path found: {:?}", path);

                // Return the first patch in the sequence
                let (first_from, first_to) = path[0];
                let url = PatchApiManager::get_patch_url_static(
                    channel,
                    std::env::consts::OS,
                    get_arch_name(),
                    first_from,
                    first_to,
                )
                .await?;
                return Ok((url, first_from, "sequential".to_string()));
            } else {
                println!("[Downloader] ✗ Sequential path not available");
            }
        }

        // Strategy 3: Complete patch from 0 -> to
        println!(
            "[Downloader] Strategy 3: Trying complete patch 0->{}",
            to_version
        );
        match PatchApiManager::get_patch_url_static(
            channel,
            std::env::consts::OS,
            get_arch_name(),
            0,
            to_version,
        )
        .await
        {
            Ok(url) => {
                println!("[Downloader] ✓ Complete patch 0->{} available", to_version);
                return Ok((url, 0, "complete".to_string()));
            }
            Err(_) => {
                anyhow::bail!(
                    "All strategies failed. Direct: {}->{} failed. Sequential failed. Complete 0->{} failed",
                    from_version,
                    to_version,
                    to_version
                );
            }
        }
    }

    /// Downloads a patch for the specified version range with multi-level fallback
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

        // Multi-level fallback strategy
        let (patch_url, actual_from_version, strategy) = self
            .try_patch_strategies(channel, from_version, to_version)
            .await?;

        let filename = format!(
            "{}~{}-{}-{}.pwr",
            actual_from_version,
            to_version,
            std::env::consts::OS,
            get_arch_name()
        );
        let patch_path = cache_dir.join(&filename);

        let should_download = if !patch_path.exists() {
            true
        } else {
            let meta = std::fs::metadata(&patch_path)?;
            if meta.len() == 0 {
                let _ = tokio::fs::remove_file(&patch_path).await;
                true
            } else {
                false
            }
        };

        if should_download {
            progress_callback(
                "patch".to_string(),
                0.0,
                format!(
                    "Downloading patch {}→{} ({})...",
                    actual_from_version, to_version, strategy
                ),
                0,
                0,
                None,
                Some(0),
            );

            #[cfg(feature = "security")]
            {
                // SECURITY: Intentar descarga segura con cada provider en orden de prioridad
                // No hardcodear un provider específico - tratar todos igual
                let providers: Vec<Box<dyn PatchProvider>> = vec![
                    Box::new(Provider0::new()),
                    Box::new(Provider1::new()),
                    Box::new(Provider2::new()),
                    Box::new(Provider3::new()),
                ];

                let cancel_token_clone = cancel_token.clone().unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
                let mut download_success = false;
                let mut last_error: Option<anyhow::Error> = None;

                for provider in providers {
                    // Verificar disponibilidad antes de intentar
                    if !provider.is_available().await {
                        println!("[Downloader] Provider {} not available, skipping", provider.name());
                        continue;
                    }

                    let progress_cb = Box::new({
                        let from_v = actual_from_version;
                        let to_v = to_version;
                        let progress_callback_clone = progress_callback.clone();
                        move |pct: f64, total: u64, downloaded: u64| {
                            let speed_str = if downloaded > 0 {
                                format!("{:.1} MB/s", downloaded as f64 / 1_048_576.0)
                            } else {
                                "Connecting...".to_string()
                            };
                            progress_callback_clone(
                                "patch".to_string(),
                                pct,
                                format!("{} - {}→{}", speed_str, from_v, to_v),
                                total,
                                downloaded,
                                None,
                                Some(0),
                            );
                        }
                    });

                    match provider
                        .download_patch_secure(
                            channel,
                            std::env::consts::OS,
                            get_arch_name(),
                            actual_from_version,
                            to_version,
                            &patch_path,
                            cancel_token_clone.clone(),
                            progress_cb,
                        )
                        .await
                    {
                        Ok(()) => {
                            println!("[Downloader] ✓ Download successful from provider {}", provider.name());
                            download_success = true;
                            break;
                        }
                        Err(e) => {
                            println!("[Downloader] ✗ Provider {} failed: {}", provider.name(), e);
                            last_error = Some(e);
                            // Continuar con el siguiente provider
                        }
                    }
                }

                if !download_success {
                    // Si todos fallaron, reportar el último error
                    if let Some(e) = last_error {
                        return Err(e);
                    } else {
                        anyhow::bail!("No providers available for download");
                    }
                }
            }

            #[cfg(not(feature = "security"))]
            {
                let progress_callback_clone = progress_callback.clone();
                let from_v = actual_from_version;
                let to_v = to_version;
                crate::download_file(
                    &patch_url,
                    &patch_path,
                    move |_phase, pct, speed, total, downloaded, eta, _step| {
                        progress_callback_clone(
                            "patch".to_string(),
                            pct,
                            format!("{} - {}→{}", speed, from_v, to_v),
                            total,
                            downloaded,
                            eta,
                            Some(0),
                        );
                    },
                    cancel_token.clone(),
                )
                .await?;
            }

            progress_callback(
                "patch".to_string(),
                100.0,
                format!(
                    "Patch {}→{} downloaded ({})",
                    actual_from_version, to_version, strategy
                ),
                0,
                0,
                None,
                Some(0),
            );
        }

        let checker = IntegrityChecker::new();

        let progress_callback_clone = progress_callback.clone();
        let cancel_token_clone = cancel_token.clone();

        let integrity_callback = move |p: f64, m: &str| {
            let status = if m == "verifying_checksum" {
                "Verifying integrity..."
            } else {
                m
            };

            progress_callback_clone(
                "verify".to_string(),
                p * 100.0,
                status.to_string(),
                0,
                0,
                None,
                Some(1),
            );
        };

        match checker
            .verify_download_integrity(
                &patch_path,
                None,
                None,
                Some(integrity_callback),
                cancel_token_clone,
            )
            .await?
        {
            integrity_res if integrity_res.is_valid() => {
                println!("[Downloader] Patch integrity verified successfully");
            }
            integrity_res => {
                let _ = tokio::fs::remove_file(&patch_path).await;
                anyhow::bail!("Integrity check failed: {:?}", integrity_res.errors);
            }
        }

        Ok(patch_path)
    }

    /// Downloads complete version
    pub async fn download_complete_version(
        &self,
        channel: &str,
        target_version: i32,
        progress_callback: ProgressCallback,
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> Result<PathBuf> {
        self.download_patch(channel, 0, target_version, progress_callback, cancel_token)
            .await
    }

    /// Checks if patch is cached
    pub async fn is_patch_cached(&self, from_version: i32, to_version: i32) -> Result<bool> {
        let cache_dir = crate::config::get_cache_dir("patches").await;

        // First check the requested patch (from->to)
        let filename = format!(
            "{}~{}-{}-{}.pwr",
            from_version,
            to_version,
            std::env::consts::OS,
            get_arch_name()
        );
        let patch_path = cache_dir.join(&filename);
        if patch_path.exists() {
            return Ok(true);
        }

        // If from > 0, also check fallback patch (0->to)
        if from_version > 0 {
            let fallback_filename = format!(
                "0~{}-{}-{}.pwr",
                to_version,
                std::env::consts::OS,
                get_arch_name()
            );
            let fallback_patch_path = cache_dir.join(&fallback_filename);
            if fallback_patch_path.exists() {
                return Ok(true);
            }
        }

        Ok(false)
    }
}
