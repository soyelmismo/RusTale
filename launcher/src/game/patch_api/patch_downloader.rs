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

        // Download if not cached or if cached and potentially corrupted
        let should_download = if !patch_path.exists() {
            true
        } else {
            // Quick check: if file is 0 bytes, it's definitely corrupted
            let meta = std::fs::metadata(&patch_path)?;
            if meta.len() == 0 {
                println!("[Downloader] Cached patch is empty, re-downloading...");
                let _ = tokio::fs::remove_file(&patch_path).await;
                true
            } else {
                false
            }
        };

        if should_download {
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
                cancel_token.clone(),
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
        } else {
            progress_callback(
                "patch",
                10.0,
                &format!("Verifying cached patch {}→{}...", from_version, to_version),
                0,
                0,
                None,
                Some(0),
            );
        }

        // ALWAYS validate patch integrity (either newly downloaded or from cache)
        let checker = super::integrity_checker::IntegrityChecker::new();
        
        // First, validate patch format
        if let Ok(format_res) = checker.validate_patch_format(&patch_path) {
            if !format_res.is_valid() {
                println!("[Downloader] Cached patch has invalid format, deleting: {:?}", format_res.errors);
                let _ = tokio::fs::remove_file(&patch_path).await;
                anyhow::bail!("Patch file is not a valid PWR/ZIP format");
            }
            println!("[Downloader] Verified patch format: {:?}", format_res.format);
        }
        
        let progress_callback_clone = progress_callback.clone();
        let cancel_token_clone = cancel_token.clone();
        
        let integrity_callback = move |p: f64, m: &str| {
            // Bridge integrity progress to main progress callback
            let status = if m == "verifying_checksum" { 
                "Verifying integrity..." 
            } else { 
                m 
            };
            
            progress_callback_clone(
                "verify",
                p * 100.0,
                status,
                0,
                0,
                None,
                Some(1),
            );
        };

        match checker.verify_download_integrity(
            &patch_path, 
            None, 
            None, 
            Some(integrity_callback),
            cancel_token_clone
        ).await? {
            integrity_res if integrity_res.is_valid() => {
                println!("[Downloader] Patch integrity verified successfully");
            },
            integrity_res => {
                println!("[Downloader] Patch integrity check failed: {:?}", integrity_res.errors);
                let _ = tokio::fs::remove_file(&patch_path).await;
                anyhow::bail!("Integrity check failed for cached/downloaded patch: {:?}", integrity_res.errors);
            }
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
