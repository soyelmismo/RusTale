use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};

use super::PatchDownloader;
use crate::game::progress::ProgressCallback;

/// Shared cache manager for patch downloads
/// This ensures both client and server use the same cache files
pub struct SharedCacheManager {}

impl SharedCacheManager {
    pub fn new() -> Self {
        Self {}
    }

    /// Get or download a patch file (shared between client and server)
    pub async fn get_or_download_patch(
        &self,
        client: &reqwest::Client,
        channel: &str,
        from_version: i32,
        to_version: i32,
        progress_callback: ProgressCallback,
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> Result<PathBuf> {
        // Use the downloader's built-in check
        let downloader = PatchDownloader::new();
        if downloader.is_patch_cached(from_version, to_version).await.unwrap_or(false) {
            let cached_path = self.get_cached_patch_path(from_version, to_version).await?;
            if let Some(path) = cached_path {
                progress_callback("cache", 100.0, "Using cached patch", 0, 0, None, Some(0));
                return Ok(path);
            }
        }

        // Download patch using the new patch API system
        downloader
            .download_patch(
                client,
                channel,
                from_version,
                to_version,
                progress_callback,
                cancel_token,
            )
            .await
    }

    /// Get cached patch path if exists
    pub async fn get_cached_patch_path(
        &self,
        from_version: i32,
        to_version: i32,
    ) -> Result<Option<PathBuf>> {
        let cache_dir = crate::config::get_cache_dir("patches").await;
        let filename = format!(
            "{}~{}-{}-{}.pwr",
            from_version,
            to_version,
            std::env::consts::OS,
            super::get_arch_name()
        );
        let patch_path = cache_dir.join(&filename);

        if patch_path.exists() {
            Ok(Some(patch_path))
        } else {
            Ok(None)
        }
    }

    /// Clean up old patches from cache
    pub async fn cleanup_old_patches(&self) -> Result<usize> {
        let cache_dir = crate::config::get_cache_dir("patches").await;

        if !cache_dir.exists() {
            return Ok(0);
        }

        let mut cleaned = 0;
        let mut entries = tokio::fs::read_dir(&cache_dir)
            .await
            .context("Failed to read cache directory")?;
        let mut entries_vec = Vec::new();

        while let Ok(Some(entry)) = entries.next_entry().await {
            entries_vec.push(entry);
        }

        // Collect entries with their metadata
        let mut entries_with_metadata = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Ok(metadata) = entry.metadata().await {
                let modified_time = metadata.modified().unwrap_or(std::time::UNIX_EPOCH);
                entries_with_metadata.push((entry, modified_time));
            }
        }

        // Sort by modification time (oldest first)
        entries_with_metadata.sort_by_key(|(_, time)| *time);

        // Keep only the latest 50 patches to avoid cache bloat
        let max_patches = 50;
        if entries_with_metadata.len() > max_patches {
            for (entry, modified_time) in entries_with_metadata
                .iter()
                .take(entries_with_metadata.len() - max_patches)
            {
                let age = std::time::SystemTime::now()
                    .duration_since(*modified_time)
                    .unwrap_or(std::time::Duration::ZERO)
                    .as_secs();

                // Only delete patches older than 30 days
                if age > 30 * 24 * 60 * 60 {
                    if let Err(e) = tokio::fs::remove_file(&entry.path()).await {
                        eprintln!(
                            "Failed to remove old patch {}: {}",
                            entry.path().display(),
                            e
                        );
                    } else {
                        cleaned += 1;
                    }
                }
            }
        }

        Ok(cleaned)
    }

    /// Get cache statistics
    pub async fn get_cache_stats(&self) -> Result<CacheStats> {
        let cache_dir = crate::config::get_cache_dir("patches").await;

        if !cache_dir.exists() {
            return Ok(CacheStats::default());
        }

        let mut total_size = 0;
        let mut file_count = 0;
        let mut oldest_age = u64::MAX;
        let mut newest_age = 0;

        let mut entries = tokio::fs::read_dir(&cache_dir)
            .await
            .context("Failed to read cache directory")?;

        while let Ok(Some(entry)) = entries.next_entry().await {
            file_count += 1;

            if let Ok(metadata) = entry.metadata().await {
                let size = metadata.len();
                let age = std::time::SystemTime::now()
                    .duration_since(metadata.modified().unwrap_or(std::time::UNIX_EPOCH))
                    .unwrap_or(std::time::Duration::ZERO)
                    .as_secs();

                total_size += size;
                oldest_age = oldest_age.min(age);
                newest_age = newest_age.max(age);
            }
        }

        Ok(CacheStats {
            file_count,
            total_size,
            oldest_age_days: oldest_age / (24 * 60 * 60),
            newest_age_days: newest_age / (24 * 60 * 60),
        })
    }
}

/// Cache statistics
#[derive(Debug, Default)]
pub struct CacheStats {
    pub file_count: usize,
    pub total_size: u64,
    pub oldest_age_days: u64,
    pub newest_age_days: u64,
}

impl CacheStats {
    pub fn size_formatted(&self) -> String {
        crate::game::patch_api::utils::format_bytes(self.total_size)
    }
}

use std::sync::OnceLock;

/// Global shared cache instance
static SHARED_CACHE: OnceLock<SharedCacheManager> = OnceLock::new();

/// Initialize the shared cache system
pub fn init_shared_cache() {
    SHARED_CACHE.get_or_init(|| SharedCacheManager::new());
}

/// Get the global shared cache instance
pub fn get_shared_cache() -> &'static SharedCacheManager {
    SHARED_CACHE.get_or_init(|| SharedCacheManager::new())
}
