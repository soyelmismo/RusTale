use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::{fs, io};
use zip::ZipArchive;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PatchManifest {
    pub mod_id: String,
    pub mod_name: String,
    pub install_date: chrono::DateTime<chrono::Utc>,
    pub enabled: bool,
    pub backups: Vec<(String, String)>, // (game path, relative backup path)
    pub added_files: Vec<String>,       // New files added
}

/// Install a new ZIP patch (Initial phase)
pub fn install_new_patch(
    zip_source_path: PathBuf,
    game_root_dir: PathBuf,     // .../latest/
    core_patches_root: PathBuf, // .../latest/CorePatches/
    mod_name: String,
) -> Result<()> {
    let mod_id = uuid::Uuid::new_v4().to_string();
    let patch_dir = core_patches_root.join(&mod_id);
    let backup_dir = patch_dir.join("backup");
    let stored_zip_path = patch_dir.join("source.zip");

    fs::create_dir_all(&backup_dir).context("Creating patch backup dir")?;

    // 1. Store the source ZIP for future activations
    fs::copy(&zip_source_path, &stored_zip_path).context("Storing source zip")?;

    // 2. Apply patch logic
    apply_patch_logic(
        &stored_zip_path,
        &game_root_dir,
        &backup_dir,
        &patch_dir,
        mod_id,
        mod_name,
    )?;

    Ok(())
}

/// Internal logic to find "Client/" or "Server/" deep within the ZIP and extract it
fn apply_patch_logic(
    zip_path: &Path,
    game_root_dir: &Path,
    backup_dir: &Path,
    patch_dir: &Path,
    mod_id: String,
    mod_name: String,
) -> Result<()> {
    let file = fs::File::open(zip_path)?;
    let mut archive = ZipArchive::new(file)?;

    // 1. Find prefix (where game folders start)
    let prefix = find_smart_prefix(&mut archive).unwrap_or_default();
    println!("[ZipMods] Base prefix detected: '{}'", prefix);

    let mut backups = Vec::new();
    let mut added_files = Vec::new();

    // Phase 1: Identification and Backup
    for i in 0..archive.len() {
        let file = archive.by_index(i)?;
        let raw_name = file.name();

        // CRITICAL: Get clean path relative to the game
        // If prefix is "install/", and file is "install/manifest.json", relative is "manifest.json"
        // If prefix is "" and file is "manifest.json", relative is "manifest.json"
        let relative_path_str = if prefix.is_empty() {
            raw_name
        } else if raw_name.starts_with(&prefix) {
            &raw_name[prefix.len()..]
        } else {
            continue; // Not under the detected prefix
        };

        // STRICT FILTER: Only allow files that start with known game folders
        // This discards "manifest.json", "README.md", "icon.png" at the mod root.
        if !is_game_file(relative_path_str) {
            // println!("[ZipMods] Ignoring non-relevant file: {}", relative_path_str);
            continue;
        }

        // Ignore empty folders/root
        if relative_path_str.is_empty() || relative_path_str.ends_with('/') {
            continue;
        }

        let target_path = game_root_dir.join(relative_path_str);

        // Backup if exists
        if target_path.exists() {
            let backup_target = backup_dir.join(relative_path_str);
            if let Some(p) = backup_target.parent() {
                fs::create_dir_all(p)?;
            }
            if target_path.is_file() {
                fs::copy(&target_path, &backup_target)?;
                backups.push((relative_path_str.to_string(), relative_path_str.to_string()));
            }
        } else {
            added_files.push(relative_path_str.to_string());
        }
    }

    // Phase 2: Real Installation
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let raw_name = file.name();

        let relative_path_str = if prefix.is_empty() {
            raw_name
        } else if raw_name.starts_with(&prefix) {
            &raw_name[prefix.len()..]
        } else {
            continue;
        };

        if !is_game_file(relative_path_str) || relative_path_str.ends_with('/') {
            continue;
        }

        let target_path = game_root_dir.join(relative_path_str);

        if let Some(p) = target_path.parent() {
            fs::create_dir_all(p)?;
        }

        // We only extract files, folders are created implicitly above
        if file.is_file() {
            let mut outfile = fs::File::create(&target_path)?;
            io::copy(&mut file, &mut outfile)?;
        }
    }

    // Save Manifest
    let manifest = PatchManifest {
        mod_id,
        mod_name,
        install_date: chrono::Utc::now(),
        enabled: true,
        backups,
        added_files,
    };

    let json = serde_json::to_string_pretty(&manifest)?;
    fs::write(patch_dir.join("manifest.json"), json)?;

    Ok(())
}

/// Critical helper function to filter out garbage
fn is_game_file(path: &str) -> bool {
    // Only accept paths that explicitly start with known game folders
    path.starts_with("Client/") || path.starts_with("Server/") || path.starts_with("Shared/")
}

/// Disable a patch: Restore backups and delete new files. DOES NOT DELETE the source ZIP.
pub fn disable_patch(
    game_root_dir: PathBuf,
    core_patches_root: PathBuf,
    mod_id: &str,
) -> Result<()> {
    let patch_dir = core_patches_root.join(mod_id);
    let manifest_path = patch_dir.join("manifest.json");
    let backup_dir = patch_dir.join("backup");

    if !manifest_path.exists() {
        anyhow::bail!("Manifest missing for {}", mod_id);
    }

    let mut manifest: PatchManifest = serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
    if !manifest.enabled {
        return Ok(());
    }

    // 1. Delete added files
    for added in &manifest.added_files {
        let p = game_root_dir.join(added);
        if p.exists() && p.is_file() {
            fs::remove_file(p).ok();
        }
    }

    // 2. Restore backups
    for (rel_game, rel_backup) in &manifest.backups {
        let source = backup_dir.join(rel_backup);
        let target = game_root_dir.join(rel_game);

        if source.exists() {
            if let Some(p) = target.parent() {
                fs::create_dir_all(p)?;
            }
            fs::copy(&source, &target)?;
        }
    }

    // 3. Update manifest
    manifest.enabled = false;
    fs::write(manifest_path, serde_json::to_string_pretty(&manifest)?)?;

    Ok(())
}

/// Reactivate a patch using the stored source.zip
pub fn enable_patch(
    game_root_dir: PathBuf,
    core_patches_root: PathBuf,
    mod_id: &str,
) -> Result<()> {
    let patch_dir = core_patches_root.join(mod_id);
    let manifest_path = patch_dir.join("manifest.json");
    let source_zip = patch_dir.join("source.zip");
    let backup_dir = patch_dir.join("backup");

    if !manifest_path.exists() || !source_zip.exists() {
        anyhow::bail!("Corrupted folder for {}", mod_id);
    }

    let manifest: PatchManifest = serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
    if manifest.enabled {
        return Ok(());
    }

    // Clean old backups
    if backup_dir.exists() {
        fs::remove_dir_all(&backup_dir)?;
    }
    fs::create_dir_all(&backup_dir)?;

    // Re-apply (will generate new backups of the current state)
    apply_patch_logic(
        &source_zip,
        &game_root_dir,
        &backup_dir,
        &patch_dir,
        manifest.mod_id,
        manifest.mod_name,
    )?;

    Ok(())
}

pub fn uninstall_patch(
    game_root_dir: PathBuf,
    core_patches_root: PathBuf,
    mod_id: &str,
) -> Result<()> {
    disable_patch(game_root_dir.clone(), core_patches_root.clone(), mod_id)?;
    let patch_dir = core_patches_root.join(mod_id);
    if patch_dir.exists() {
        fs::remove_dir_all(patch_dir)?;
    }
    Ok(())
}

pub fn list_patches(core_patches_root: PathBuf) -> Result<Vec<PatchManifest>> {
    let mut mods = Vec::new();
    if !core_patches_root.exists() {
        return Ok(mods);
    }

    for entry in fs::read_dir(core_patches_root)? {
        let entry = entry?;
        if entry.path().is_dir() {
            let manifest_path = entry.path().join("manifest.json");
            if manifest_path.exists() {
                if let Ok(content) = fs::read_to_string(manifest_path) {
                    if let Ok(m) = serde_json::from_str::<PatchManifest>(&content) {
                        mods.push(m);
                    }
                }
            }
        }
    }
    // Sort by date
    mods.sort_by(|a, b| b.install_date.cmp(&a.install_date));
    Ok(mods)
}

/// Recursively searches for where "Client" or "Server" folders begin
fn find_smart_prefix(archive: &mut ZipArchive<fs::File>) -> Option<String> {
    for i in 0..archive.len() {
        if let Ok(file) = archive.by_index(i) {
            let name = file.name();

            // Priority: Detect deep nested folders
            if let Some(idx) = name.find("/Client/") {
                return Some(name[..idx + 1].to_string());
            }

            // Simple root
            if name.starts_with("Client/") {
                return Some("".to_string());
            }
        }
    }
    None
}

/// Verifies if a ZIP is a valid patch
pub fn is_patch_mod(zip_path: &Path) -> bool {
    let file = fs::File::open(zip_path).ok();
    if let Some(f) = file {
        if let Ok(mut archive) = ZipArchive::new(f) {
            // A ZIP is a patch if it contains Client/ or Server/ somewhere
            return find_smart_prefix(&mut archive).is_some();
        }
    }
    false
}
