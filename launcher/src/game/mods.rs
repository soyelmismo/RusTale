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

    async fn scan(dir: &Path, enabled: bool, list: &mut Vec<ModInfo>) -> Result<()> {
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
                list.push(ModInfo {
                    name,
                    file_name: path.file_name().unwrap().to_string_lossy().to_string(),
                    enabled,
                    path,
                    size: meta.len(),
                });
            }
        }
        Ok(())
    }

    scan(&mods_dir, true, &mut mods).await?;
    scan(&disabled_dir, false, &mut mods).await?;
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
