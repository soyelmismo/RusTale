use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::fs;

use crate::config;

/// Inicializa la aplicacion: crea directorios necesarios y realiza limpiezas previas.
/// Equivalente a startup() y env.CreateFolders() en Go.
pub async fn initialize() -> Result<()> {
    // Initialize the new patch API system
    crate::game::patch_api::PatchApiFrontend::get_instance();
    
    let base_dir = config::get_app_dir();

    // Lista de carpetas a crear
    let folders = vec![
        base_dir.clone(),
        config::get_server_root_dir(), // RusTale/server
        config::get_identity_dir(),    // RusTale/server/identity
        base_dir.join("cache"),
        base_dir.join("cache").join("images"),
        base_dir.join("cache").join("patches"),
        base_dir.join("cache").join("jre"),
        base_dir.join("logs"),
        base_dir.join("tools"),
        base_dir.join("tools").join("butler"),
        base_dir.join("tools").join("jre")
    ];

    for folder in folders {
        if !folder.exists() {
            fs::create_dir_all(&folder)
                .await
                .context(format!("Failed to create directory: {:?}", folder))?;
        }
    }

    // Limpieza de arranque
    cleanup_launcher(&base_dir).await;

    Ok(())
}

/// Limpieza de archivos temporales y logs viejos.
/// Equivalente a env.CleanupLauncher() en Go.
async fn cleanup_launcher(base_dir: &PathBuf) {
    // --- NUEVO: Limpiar archivos parciales en caches ---
    let patches_cache = base_dir.join("cache").join("patches");
    cleanup_partials(&patches_cache).await;

    let jre_cache = base_dir.join("cache").join("jre");
    cleanup_partials(&jre_cache).await;

    // Rotacion basica de logs (Placeholder para logica mas compleja)
    let logs_dir = base_dir.join("logs");
    cleanup_old_files(&logs_dir, 7).await; // Borrar logs de mas de 7 dias
}

// Funcion auxiliar nueva
async fn cleanup_partials(dir: &PathBuf) {
    if !dir.exists() {
        return;
    }
    if let Ok(mut entries) = fs::read_dir(dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            // Borrar si termina en .downloading o .tmp
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
