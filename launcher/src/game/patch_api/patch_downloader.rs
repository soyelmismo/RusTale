use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};

use super::PatchApiManager;
use super::utils::*;
use crate::game::progress::ProgressCallback;

/// Downloader for game patches using the new patch API system
#[derive(Clone)]
pub struct PatchDownloader {}

impl PatchDownloader {
    pub fn new() -> Self {
        Self {}
    }

    /// Downloads a patch for the specified version range
    pub async fn download_patch(
        &self,
        client: &reqwest::Client,
        channel: &str,
        from_version: i32,
        to_version: i32,
        progress_callback: ProgressCallback,
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> Result<PathBuf> {
        let cache_dir = crate::config::get_cache_dir("patches").await;
        tokio::fs::create_dir_all(&cache_dir).await?;

        // Get patch URL from patch API
        let patch_url = PatchApiManager::get_patch_url_static(
            channel,
            std::env::consts::OS,
            get_arch_name(),
            from_version,
            to_version,
        )
        .await
        .context("Failed to get patch download URL")?;

        // Generate filename
        let filename = format!(
            "{}~{}-{}-{}.pwr",
            from_version,
            to_version,
            std::env::consts::OS,
            get_arch_name()
        );
        let patch_path = cache_dir.join(&filename);

        // Download if not cached
        if !patch_path.exists() {
            let cancel_token_clone = cancel_token.clone(); // Clone before first use
            
            progress_callback(
                "patch",
                0.0,
                &format!("Downloading patch {}→{}...", from_version, to_version),
                0,
                0,
                None,
                Some(0),
            );

            crate::game::downloader::download_file(
                client,
                &patch_url,
                &patch_path,
                |pct, speed, total, downloaded, eta| {
                    progress_callback(
                        "patch",
                        pct as f64,
                        &format!("{} - {}→{}", speed, from_version, to_version),
                        total,
                        downloaded,
                        eta,
                        Some(0),
                    );
                },
                cancel_token,
            )
            .await?;

            progress_callback(
                "patch",
                100.0,
                &format!("Patch {}→{} downloaded", from_version, to_version),
                0,
                0,
                None,
                Some(0),
            );

            // Validate downloaded patch integrity with high-fidelity checks
            let checker = super::integrity_checker::IntegrityChecker::new();
            
            // First, validate patch format
            if let Ok(format_res) = checker.validate_patch_format(&patch_path) {
                if !format_res.is_valid() {
                    let _ = tokio::fs::remove_file(&patch_path).await;
                    anyhow::bail!("Downloaded file is not a valid PWR/ZIP patch");
                }
                println!("[Downloader] Verified patch format: {:?}", format_res.format);
            }
            
            let progress_callback_clone = progress_callback.clone();
            let integrity_callback = move |p: f64, m: &str| {
                // Bridge integrity progress to main progress callback
                // Map integrity messages to proper phase and status
                let phase = "verify"; // Use verify phase for integrity checks
                let status = if m == "verifying_checksum" { 
                    "Verifying integrity..." 
                } else { 
                    m 
                };
                
                progress_callback_clone(
                    phase,
                    p * 100.0, // Convert to percentage for compatibility
                    status,
                    0,
                    0,
                    None,
                    Some(1), // Step 1 of integrity verification
                );
            };
            let integrity_res = checker.verify_download_integrity(
                &patch_path, 
                None, // Add signature path if available in future
                None, 
                Some(integrity_callback),
                cancel_token_clone
            ).await?;

            if !integrity_res.is_valid() {
                let _err = tokio::fs::remove_file(&patch_path).await;
                anyhow::bail!("Integrity check failed: {:?}", integrity_res.errors);
            }
        } else {
            progress_callback(
                "patch",
                100.0,
                &format!("Patch {}→{} found in cache", from_version, to_version),
                0,
                0,
                None,
                Some(0),
            );
        }

        Ok(patch_path)
    }

    /// Downloads complete version (from 0 to target)
    pub async fn download_complete_version(
        &self,
        client: &reqwest::Client,
        channel: &str,
        target_version: i32,
        progress_callback: ProgressCallback,
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> Result<PathBuf> {
        self.download_patch(
            client,
            channel,
            0,
            target_version,
            progress_callback,
            cancel_token,
        )
        .await
    }

    /// Checks if patch is cached
    pub async fn is_patch_cached(
        &self,
        from_version: i32,
        to_version: i32,
    ) -> Result<bool> {
        let cache_dir = crate::config::get_cache_dir("patches").await;
        let filename = format!(
            "{}~{}-{}-{}.pwr",
            from_version,
            to_version,
            std::env::consts::OS,
            get_arch_name()
        );
        let patch_path = cache_dir.join(&filename);
        Ok(patch_path.exists())
    }
}
