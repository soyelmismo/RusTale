use crate::game::mods_api::ModProvider;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;

/// Request structure for mod installation operations
/// This encapsulates all metadata needed to install a mod cleanly
#[derive(Debug, Clone)]
pub struct ModInstallationRequest {
    pub mod_id: String,
    pub mod_name: String,
    pub remote_id: Option<String>,
    pub file_id: Option<String>,
    pub provider: Option<ModProvider>,
    pub summary: Option<String>,
    pub logo_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModInfo {
    pub name: String,
    pub file_name: String,
    pub enabled: bool,
    pub path: PathBuf,
    pub size: u64,
    pub metadata: Option<InstalledModMetadata>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct InstalledModMetadata {
    pub file_name: String, // Clave primaria para relacionar con el disco
    pub mod_name: String,  // Para UI rapida
    pub provider: ModProvider,
    pub mod_id: String,  // ID Remoto (ej: "345123")
    pub file_id: String, // ID de la version instalada (ej: "888111")
    #[serde(default = "default_true")]
    pub enabled: bool, // Estado del mod
    pub summary: Option<String>,
    pub logo_url: Option<String>,
    pub install_date: chrono::DateTime<chrono::Utc>,
    pub update_available: Option<String>, // None o ID de la nueva version
}

pub async fn ensure_mod_dirs(base_dir: &Path, channel: &str, version: &str) -> (PathBuf, PathBuf) {
    let paths = crate::game::GamePaths::new(base_dir.to_path_buf());
    let mods_dir = paths.mods_dir(channel, version);
    let disabled_dir = paths.disabled_mods_dir(channel, version);

    if !mods_dir.exists() {
        let _ = fs::create_dir_all(&mods_dir).await;
    }
    if !disabled_dir.exists() {
        let _ = fs::create_dir_all(&disabled_dir).await;
    }

    (mods_dir, disabled_dir)
}

pub async fn list_mods(base_dir: &Path, channel: &str, version: &str) -> Result<Vec<ModInfo>> {
    let (mods_dir, disabled_dir) = ensure_mod_dirs(base_dir, channel, version).await;
    let mut mods = Vec::new();

    // Cargar el manifiesto de metadatos
    let manifest = load_manifest(base_dir, channel, version).await;
    let manifest_map: std::collections::HashMap<String, InstalledModMetadata> = manifest
        .into_iter()
        .map(|m| (m.file_name.clone(), m))
        .collect();

    async fn scan(
        dir: &Path,
        enabled: bool,
        list: &mut Vec<ModInfo>,
        manifest_map: &std::collections::HashMap<String, InstalledModMetadata>,
    ) -> Result<()> {
        if !dir.exists() {
            return Ok(());
        }
        let mut entries = fs::read_dir(dir).await?;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            // ignore folders and hidden files, accept zip and jar
            if path.is_file()
                && !name.starts_with('.')
                && (name.ends_with(".jar") || name.ends_with(".zip"))
            {
                let meta = entry.metadata().await?;
                let metadata = manifest_map.get(&name).cloned();
                list.push(ModInfo {
                    name,
                    file_name: path.file_name().unwrap().to_string_lossy().to_string(),
                    enabled,
                    path,
                    size: meta.len(),
                    metadata,
                });
            }
        }
        Ok(())
    }

    scan(&mods_dir, true, &mut mods, &manifest_map).await?;
    scan(&disabled_dir, false, &mut mods, &manifest_map).await?;
    mods.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(mods)
}

pub async fn toggle_mod(
    base_dir: &Path,
    channel: &str,
    version: &str,
    mod_info: &ModInfo,
) -> Result<()> {
    let (mods_dir, disabled_dir) = ensure_mod_dirs(base_dir, channel, version).await;
    let target_dir = if mod_info.enabled {
        &disabled_dir
    } else {
        &mods_dir
    };
    let target_path = target_dir.join(&mod_info.file_name);

    fs::rename(&mod_info.path, &target_path)
        .await
        .context("Moving mod file")?;
    Ok(())
}

pub async fn delete_mod(mod_info: &ModInfo) -> Result<()> {
    if mod_info.path.exists() {
        fs::remove_file(&mod_info.path).await?;
    }
    Ok(())
}

// Funcion para guardar el manifiesto
pub async fn save_manifest(
    base_dir: &std::path::Path,
    channel: &str,
    version: &str,
    metadata: &Vec<InstalledModMetadata>,
) -> anyhow::Result<()> {
    let paths = crate::game::GamePaths::new(base_dir.to_path_buf());
    let mods_dir = paths.mods_dir(channel, version);
    if !mods_dir.exists() {
        tokio::fs::create_dir_all(&mods_dir).await?;
    }
    let manifest_path = mods_dir.join("mods_manifest.json");
    let json = serde_json::to_string_pretty(metadata)?;
    tokio::fs::write(manifest_path, json).await?;
    Ok(())
}

// Funcion para leer manifiesto
pub async fn load_manifest(
    base_dir: &std::path::Path,
    channel: &str,
    version: &str,
) -> Vec<InstalledModMetadata> {
    let paths = crate::game::GamePaths::new(base_dir.to_path_buf());
    let manifest_path = paths.mods_dir(channel, version).join("mods_manifest.json");
    if let Ok(content) = tokio::fs::read_to_string(manifest_path).await {
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        Vec::new()
    }
}
