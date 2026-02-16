use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};

use super::PatchApiManager;
use crate::game::paths::GamePaths;

/// Downloader for game patches using the new patch API system
#[derive(Clone)]
pub struct PatchDownloader {
    api_manager: Arc<PatchApiManager>,
}

impl PatchDownloader {
    pub fn new(api_manager: Arc<PatchApiManager>) -> Self {
        Self { api_manager }
    }

    /// Downloads a patch for the specified version range
    pub async fn download_patch(
        &self,
        client: &reqwest::Client,
        base_dir: &PathBuf,
        channel: &str,
        from_version: i32,
        to_version: i32,
        progress_callback: impl Fn(&str, f64, &str, u64, u64, Option<String>, Option<usize>),
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> Result<PathBuf> {
        let paths = GamePaths::new(base_dir.clone());
        let cache_dir = crate::config::get_cache_dir("patches").await;
        tokio::fs::create_dir_all(&cache_dir).await?;

        // Get patch URL from patch API
        let patch_url = self.api_manager.get_patch_url(channel, std::env::consts::OS, get_arch_name(), from_version, to_version).await
            .context("Failed to get patch download URL")?;

        // Generate filename
        let filename = format!("{}~{}-{}-{}.pwr", from_version, to_version, std::env::consts::OS, get_arch_name());
        let patch_path = cache_dir.join(&filename);

        // Download if not cached
        if !patch_path.exists() {
            progress_callback("patch", 0.0, &format!("Downloading patch {}→{}...", from_version, to_version), 0, 0, None, Some(0));

            crate::game::downloader::download_file(
                client,
                &patch_url,
                &patch_path,
                |pct, speed, total, downloaded, eta| {
                    let size_info = if total > 0 {
                        format!("{} / {}", 
                            crate::game::downloader::format_bytes(downloaded), 
                            crate::game::downloader::format_bytes(total)
                        )
                    } else {
                        crate::game::downloader::format_bytes(downloaded)
                    };
                    
                    let eta_info = if let Some(eta_str) = &eta {
                        format!(" • ETA: {}", eta_str)
                    } else {
                        String::new()
                    };
                    
                    progress_callback(
                        "patch",
                        pct as f64,
                        &format!("{}→{}: {}{}{}", from_version, to_version, speed, size_info, eta_info),
                        total,
                        downloaded,
                        eta,
                        Some(0),
                    );
                },
                cancel_token,
            ).await?;

            progress_callback("patch", 100.0, &format!("Patch {}→{} downloaded", from_version, to_version), 0, 0, None, Some(0));
        } else {
            progress_callback("patch", 100.0, &format!("Patch {}→{} found in cache", from_version, to_version), 0, 0, None, Some(0));
        }

        Ok(patch_path)
    }

    /// Downloads patch signature for verification
    pub async fn download_patch_signature(
        &self,
        client: &reqwest::Client,
        base_dir: &PathBuf,
        channel: &str,
        from_version: i32,
        to_version: i32,
        progress_callback: impl Fn(&str, f64, &str, u64, u64, Option<String>, Option<usize>),
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> Result<PathBuf> {
        let paths = GamePaths::new(base_dir.clone());
        let cache_dir = crate::config::get_cache_dir("patches").await;
        tokio::fs::create_dir_all(&cache_dir).await?;

        // Get patch signature URL from patch API
        let sig_url = self.api_manager.get_patch_signature_url(channel, std::env::consts::OS, get_arch_name(), from_version, to_version).await
            .context("Failed to get patch signature URL")?;

        // Generate filename
        let filename = format!("{}~{}-{}-{}.pwr.sig", from_version, to_version, std::env::consts::OS, get_arch_name());
        let sig_path = cache_dir.join(&filename);

        // Download if not cached
        if !sig_path.exists() {
            progress_callback("signature", 0.0, &format!("Downloading patch signature {}→{}...", from_version, to_version), 0, 0, None, Some(1));

            crate::game::downloader::download_file(
                client,
                &sig_url,
                &sig_path,
                |pct, speed, total, downloaded, eta| {
                    progress_callback(
                        "signature",
                        pct as f64,
                        &format!("Downloading signature {}→{}... ({})", from_version, to_version, speed),
                        total,
                        downloaded,
                        eta,
                        Some(1),
                    );
                },
                cancel_token,
            ).await?;

            progress_callback("signature", 100.0, &format!("Patch signature {}→{} downloaded", from_version, to_version), 0, 0, None, Some(1));
        } else {
            progress_callback("signature", 100.0, &format!("Patch signature {}→{} found in cache", from_version, to_version), 0, 0, None, Some(1));
        }

        Ok(sig_path)
    }

    /// Downloads both patch and signature (signature is optional)
    pub async fn download_patch_with_signature(
        &self,
        client: &reqwest::Client,
        base_dir: &PathBuf,
        channel: &str,
        from_version: i32,
        to_version: i32,
        progress_callback: impl Fn(&str, f64, &str, u64, u64, Option<String>, Option<usize>),
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> Result<(PathBuf, Option<PathBuf>)> {
        // Download patch
        let patch_path = self.download_patch(
            client,
            base_dir,
            channel,
            from_version,
            to_version,
            &progress_callback,
            cancel_token.clone(),
        ).await?;

        // Try to download signature (optional - don't fail if it doesn't work)
        let sig_path = match self.download_patch_signature(
            client,
            base_dir,
            channel,
            from_version,
            to_version,
            &progress_callback,
            cancel_token,
        ).await {
            Ok(path) => {
                progress_callback("signature", 100.0, &format!("Patch signature {}→{} downloaded successfully", from_version, to_version), 0, 0, None, Some(1));
                Some(path)
            }
            Err(e) => {
                progress_callback("signature", 0.0, &format!("Failed to download patch signature {}→{}: {}. Proceeding without signature verification.", from_version, to_version, e), 0, 0, None, Some(1));
                None
            }
        };

        Ok((patch_path, sig_path))
    }

    /// Downloads complete version (from 0 to target)
    pub async fn download_complete_version(
        &self,
        client: &reqwest::Client,
        base_dir: &PathBuf,
        channel: &str,
        target_version: i32,
        progress_callback: impl Fn(&str, f64, &str, u64, u64, Option<String>, Option<usize>),
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> Result<(PathBuf, Option<PathBuf>)> {
        self.download_patch_with_signature(
            client,
            base_dir,
            channel,
            0,
            target_version,
            progress_callback,
            cancel_token,
        ).await
    }

    /// Checks if patch is cached
    pub async fn is_patch_cached(&self, base_dir: &PathBuf, from_version: i32, to_version: i32) -> Result<bool> {
        let cache_dir = crate::config::get_cache_dir("patches").await;
        let filename = format!("{}~{}-{}-{}.pwr", from_version, to_version, std::env::consts::OS, get_arch_name());
        let patch_path = cache_dir.join(&filename);
        Ok(patch_path.exists())
    }

    /// Checks if patch signature is cached
    pub async fn is_signature_cached(&self, base_dir: &PathBuf, from_version: i32, to_version: i32) -> Result<bool> {
        let cache_dir = crate::config::get_cache_dir("patches").await;
        let filename = format!("{}~{}-{}-{}.pwr.sig", from_version, to_version, std::env::consts::OS, get_arch_name());
        let sig_path = cache_dir.join(&filename);
        Ok(sig_path.exists())
    }

    /// Gets cached patch path if exists
    pub async fn get_cached_patch_path(&self, base_dir: &PathBuf, from_version: i32, to_version: i32) -> Result<Option<PathBuf>> {
        let cache_dir = crate::config::get_cache_dir("patches").await;
        let filename = format!("{}~{}-{}-{}.pwr", from_version, to_version, std::env::consts::OS, get_arch_name());
        let patch_path = cache_dir.join(&filename);
        
        if patch_path.exists() {
            Ok(Some(patch_path))
        } else {
            Ok(None)
        }
    }
}

fn get_arch_name() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    }
}
