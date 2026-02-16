use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

use super::{PatchDownloader, PatchApiManager};

/// Shared cache manager for patch downloads
/// This ensures both client and server use the same cache files
pub struct SharedCacheManager {
    api_manager: Arc<PatchApiManager>,
}

impl SharedCacheManager {
    pub fn new(api_manager: Arc<PatchApiManager>) -> Self {
        Self { api_manager }
    }

    /// Get or download a patch file (shared between client and server)
    pub async fn get_or_download_patch(
        &self,
        client: &reqwest::Client,
        base_dir: &PathBuf,
        channel: &str,
        from_version: i32,
        to_version: i32,
        progress_callback: impl Fn(&str, f64, &str, u64, u64, Option<String>, Option<usize>),
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> Result<PathBuf> {
        // Check if patch is already cached
        if let Some(cached_path) = self.get_cached_patch_path(base_dir, from_version, to_version).await? {
            if cached_path.exists() {
                progress_callback("cache", 100.0, "Using cached patch", 0, 0, None, Some(0));
                return Ok(cached_path);
            }
        }

        // Download patch using the new patch API system
        let downloader = PatchDownloader::new(self.api_manager.clone());
        downloader.download_patch(
            client,
            base_dir,
            channel,
            from_version,
            to_version,
            progress_callback,
            cancel_token,
        ).await
    }

    /// Get cached patch path if exists
    pub async fn get_cached_patch_path(&self, base_dir: &PathBuf, from_version: i32, to_version: i32) -> Result<Option<PathBuf>> {
        let cache_dir = crate::config::get_cache_dir("patches").await?;
        let filename = format!("{}~{}-{}.pwr", from_version, to_version, std::env::consts::OS, super::get_arch_name());
        let patch_path = cache_dir.join(&filename);
        
        if patch_path.exists() {
            Ok(Some(patch_path))
        } else {
            Ok(None)
        }
    }

    /// Clean up old patches from cache
    pub async fn cleanup_old_patches(&self, base_dir: &PathBuf) -> Result<usize> {
        let cache_dir = crate::config::get_cache_dir("patches").await?;
        
        if !cache_dir.exists() {
            return Ok(0);
        }

        let mut cleaned = 0;
        let mut entries = tokio::fs::read_dir(&cache_dir).await
            .context("Failed to read cache directory")?;
        let mut entries_vec = Vec::new();

        while let Ok(Some(entry)) = entries.next_entry().await {
            entries_vec.push(entry);
        }

        // Sort by modification time (oldest first)
        entries_vec.sort_by_key(|e| {
            e.metadata()
                .map(|m| m.modified().unwrap_or(std::time::UNIX_EPOCH))
                .unwrap_or(std::time::UNIX_EPOCH)
        });

        // Keep only the latest 50 patches to avoid cache bloat
        let max_patches = 50;
        if entries_vec.len() > max_patches {
            for entry in entries_vec.iter().take(entries_vec.len() - max_patches) {
                if let Ok(metadata) = entry.metadata() {
                    let age = std::time::SystemTime::now()
                        .duration_since(metadata.modified().unwrap_or(std::time::UNIX_EPOCH))
                        .as_secs();
                    
                    // Only delete patches older than 30 days
                    if age > 30 * 24 * 60 * 60 {
                        if let Err(e) = tokio::fs::remove_file(&entry.path()).await {
                            eprintln!("Failed to remove old patch {}: {}", entry.path().display(), e);
                        } else {
                            cleaned += 1;
                        }
                    }
                }
            }
        }

        Ok(cleaned)
    }

    /// Get cache statistics
    pub async fn get_cache_stats(&self, base_dir: &PathBuf) -> Result<CacheStats> {
        let cache_dir = crate::config::get_cache_dir("patches").await?;
        
        if !cache_dir.exists() {
            return Ok(CacheStats::default());
        }

        let mut total_size = 0;
        let file_count = 0;
        let mut oldest_age = u64::MAX;
        let newest_age = 0;

        let mut entries = tokio::fs::read_dir(&cache_dir).await
            .context("Failed to read cache directory")?;

        while let Ok(Some(entry)) = entries.next_entry().await {
            file_count += 1;
            
            if let Ok(metadata) = entry.metadata() {
                let size = metadata.len();
                let age = std::time::SystemTime::now()
                    .duration_since(metadata.modified().unwrap_or(std::time::UNIX_EPOCH))
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
        crate::game::downloader::format_bytes(self.total_size)
    }
}

/// Global shared cache instance
static mut SHARED_CACHE: Option<SharedCacheManager> = None;
static INIT: std::sync::Once = std::sync::Once::new();

/// Initialize the shared cache system
pub fn init_shared_cache(api_manager: Arc<PatchApiManager>) {
    INIT.call_once(|| {
        unsafe {
            SHARED_CACHE = Some(SharedCacheManager::new(api_manager));
        }
    });
}

/// Get the global shared cache instance
pub fn get_shared_cache() -> &'static SharedCacheManager {
    INIT.call_once(|| {
        let api_manager = Arc::new(PatchApiManager::new());
        unsafe {
            SHARED_CACHE = Some(SharedCacheManager::new(api_manager));
        }
    });
    unsafe { SHARED_CACHE.as_ref().unwrap() }
}
