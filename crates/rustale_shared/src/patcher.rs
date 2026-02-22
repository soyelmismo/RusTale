//! Patcher module - Apply patches to game installation
//! 
//! This module provides functionality to apply .pwr patch files.

use anyhow::Result;
pub use crate::butler::apply_pwr;

/// Cleans up the patches cache directory using the shared cache system
pub async fn clean_patches_cache(
    progress_callback: impl Fn(f32, &str, u64, u64, Option<String>, Option<usize>),
) -> Result<()> {
    crate::patch_api::get_shared_cache()
        .cleanup_old_patches()
        .await?;

    progress_callback(
        100.0,
        "Cleaned cache files",
        0,
        0,
        None,
        None,
    );
    Ok(())
}