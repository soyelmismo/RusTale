use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::fs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModInfo {
    pub name: String,
    pub file_name: String,
    pub enabled: bool,
    pub path: PathBuf,
    pub size: u64,
}

pub async fn get_mods_dirs(base_dir: &Path) -> (PathBuf, PathBuf) {
    let paths = crate::game::GamePaths::new(base_dir.to_path_buf());
    // Usamos la carpeta de usuario del juego para compatibilidad
    let user_data = paths.user_data();
    let mods_dir = user_data.join("Mods");
    let disabled_dir = user_data.join("DisabledMods");

    // Asegurar que existen
    if !mods_dir.exists() {
        let _ = fs::create_dir_all(&mods_dir).await;
        println!("[Mods] Created mods directory at: {:?}", mods_dir);
    }
    if !disabled_dir.exists() {
        let _ = fs::create_dir_all(&disabled_dir).await;
    }

    (mods_dir, disabled_dir)
}


pub async fn list_mods(base_dir: &Path) -> Result<Vec<ModInfo>> {
    let (mods_dir, disabled_dir) = get_mods_dirs(base_dir).await;
    let mut mods = Vec::new();

    println!("[Mods] Scanning for mods in: {:?}", mods_dir);

    // Helper para escanear
    async fn scan_dir(dir: &Path, enabled: bool, list: &mut Vec<ModInfo>) -> Result<()> {
        if !dir.exists() {
            return Ok(());
        }
        
        let mut entries = fs::read_dir(dir).await?;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            let name_lower = name.to_lowercase();

            // FIX: Detección insensible a mayúsculas y exclusión de archivos temporales
            if path.is_file() 
               && !name.starts_with('.') // Ignorar .DS_Store o archivos ocultos
               && (name_lower.ends_with(".jar") || name_lower.ends_with(".zip")) 
            {
                let metadata = entry.metadata().await?;
                list.push(ModInfo {
                    name: name.clone(), 
                    file_name: name.clone(),
                    enabled,
                    path,
                    size: metadata.len(),
                });

                // error: borrow of moved value: `name` value borrowed here after move
                println!("[Mods] Found mod: {}", name.clone());
            } else if path.is_file() {
                println!("[Mods] Ignored file (invalid extension): {}", name.clone());
            }
        }
        Ok(())
    }

    let _ = scan_dir(&mods_dir, true, &mut mods).await;
    let _ = scan_dir(&disabled_dir, false, &mut mods).await;

    // Ordenar alfabéticamente
    mods.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    
    println!("[Mods] Total mods found: {}", mods.len());
    
    Ok(mods)
}

pub async fn toggle_mod(base_dir: &Path, mod_info: &ModInfo) -> Result<()> {
    let (mods_dir, disabled_dir) = get_mods_dirs(base_dir).await;

    let target_dir = if mod_info.enabled {
        &disabled_dir
    } else {
        &mods_dir
    };
    let target_path = target_dir.join(&mod_info.file_name);

    fs::rename(&mod_info.path, &target_path)
        .await
        .context("Failed to move mod file")?;

    Ok(())
}


pub async fn delete_mod(mod_info: &ModInfo) -> Result<()> {
    if mod_info.path.exists() {
        fs::remove_file(&mod_info.path).await?;
    }
    Ok(())
}

