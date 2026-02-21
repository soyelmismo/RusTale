// crates/rustale_engine/src/system/lifecycle.rs

use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::fs;
use crate::system;

/// Ensures the environment is ready (directories, permissions, cleanup)
pub async fn bootstrap() -> Result<()> {
    // 1. Initialize Patch API Cache
    crate::game::patch_api::init_shared_cache();

    let base_dir = system::get_app_dir();
    
    // 2. Critical Paths
    let folders = vec![
        base_dir.clone(),
        system::get_server_root_dir(), // RusTale/server
        system::get_identity_dir(),    // RusTale/server/identity
        base_dir.join("cache"),
        base_dir.join("cache").join("images"),
        base_dir.join("cache").join("patches"),
        base_dir.join("cache").join("jre"),
        base_dir.join("logs"),
        base_dir.join("tools"),
        base_dir.join("tools").join("butler"),
        base_dir.join("tools").join("jre"),
    ];

    for folder in folders {
        if !folder.exists() {
            fs::create_dir_all(&folder).await
                .context(format!("Bootstrap: Failed to create {:?}", folder))?;
        }
    }

    // 3. Maintenance
    perform_maintenance(&base_dir).await;

    Ok(())
}

async fn perform_maintenance(base_dir: &PathBuf) {
    // --- NUEVO: Limpiar archivos parciales en caches ---
    let patches_cache = base_dir.join("cache").join("patches");
    cleanup_partials(&patches_cache).await;

    let jre_cache = base_dir.join("cache").join("jre");
    cleanup_partials(&jre_cache).await;

    // Rotacion basica de logs
    let logs_dir = base_dir.join("logs");
    cleanup_old_files(&logs_dir, 7).await; // Borrar logs de mas de 7 dias

    // Clean old patches using shared cache logic
    if let Ok(count) = crate::game::patch_api::get_shared_cache().cleanup_old_patches().await {
        println!("[Cache] Purged {} old patches", count);
    }

    // Use the patcher's cleanup logic (which might be redundant but safe)
    let _ = crate::game::patcher::clean_patches_cache(|p, msg, _, _, _, _| {
        println!("[Cleanup] {}% - {}", p, msg);
    }).await.map_err(|e| eprintln!("Cache cleanup failed: {}", e));
}

async fn cleanup_partials(dir: &PathBuf) {
    if !dir.exists() {
        return;
    }
    if let Ok(mut entries) = fs::read_dir(dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                if ext == "downloading" || ext == "tmp" {
                    let _ = fs::remove_file(path).await;
                }
            }
        }
    }
}

async fn cleanup_old_files(dir: &PathBuf, days: u64) {
    if !dir.exists() {
        return;
    }

    if let Ok(mut entries) = fs::read_dir(dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Ok(metadata) = entry.metadata().await {
                if let Ok(modified) = metadata.modified() {
                    if let Ok(elapsed) = modified.elapsed() {
                        if elapsed.as_secs() > days * 24 * 3600 {
                            let _ = fs::remove_file(entry.path()).await;
                        }
                    }
                }
            }
        }
    }
}
