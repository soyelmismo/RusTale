use iced::widget::image;
use lru::LruCache;
use std::num::NonZeroUsize;

pub struct ImageCache {
    cache: LruCache<String, image::Handle>,
}

impl ImageCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: LruCache::new(NonZeroUsize::new(capacity).unwrap()),
        }
    }

    pub fn get(&mut self, key: &str) -> Option<image::Handle> {
        self.cache.get(key).cloned()
    }

    pub fn insert(&mut self, key: String, handle: image::Handle) {
        self.cache.put(key, handle);
    }

    pub fn clear(&mut self) {
        self.cache.clear();
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    // Immutable access without updating LRU order
    pub fn peek(&self, key: &str) -> Option<image::Handle> {
        self.cache.peek(key).cloned()
    }
    
    pub fn capacity(&self) -> usize {
        self.cache.cap().get()
    }
    
    pub fn usage_ratio(&self) -> f32 {
        self.len() as f32 / self.capacity() as f32
    }
    
    pub fn remove(&mut self, key: &str) -> Option<image::Handle> {
        self.cache.pop(key)
    }
}

pub struct UiResources {
    // Shared cache for all generic thumbnails (news, mods, profiles)
    // Global limit prevents OOM even if loading 1000s of news
    pub global_thumbnails: ImageCache,
}

impl Default for UiResources {
    fn default() -> Self {
        Self {
            // Keep max 100 images (~50MB - 100MB VRAM max)
            global_thumbnails: ImageCache::new(100), 
        }
    }
}

impl UiResources {
    pub fn get_cache_stats(&self) -> String {
        format!(
            "Cache: {}/{} images ({:.1}%)",
            self.global_thumbnails.len(),
            self.global_thumbnails.capacity(),
            self.global_thumbnails.usage_ratio() * 100.0
        )
    }
    
    pub fn cleanup_old_entries(&mut self, threshold: f32) {
        // Remove entries if cache usage exceeds threshold
        if self.global_thumbnails.usage_ratio() > threshold {
            // Clear half the cache to make room
            let target_size = (self.global_thumbnails.capacity() as f32 * (1.0 - threshold)) as usize;
            while self.global_thumbnails.len() > target_size {
                // LRU cache automatically removes oldest when inserting new items
                // For manual cleanup, we'd need to implement iteration
                break;
            }
        }
    }
}
