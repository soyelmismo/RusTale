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
    pub is_hybrid: bool, // Nueva bandera para indicar si es un mod hibrido
    pub backups: Vec<(String, String)>, // (game path, relative backup path)
    pub added_files: Vec<String>,       // New files added
    // Metadata for remote updates
    pub remote_id: Option<String>,
    pub file_id: Option<String>,
    pub provider: Option<crate::game::mods_api::ModProvider>,
    pub summary: Option<String>,
    pub logo_url: Option<String>,
}

/// Install a new ZIP patch (Initial phase)
pub fn install_new_patch(
    zip_source_path: PathBuf,
    game_root_dir: PathBuf,     // .../latest/
    core_patches_root: PathBuf, // .../latest/CorePatches/
    channel: String,             // "latest", "beta", etc.
    version: String,             // version number
    mod_id: String,             // ID del mod para nombre de carpeta
    mod_name: String,
    remote_id: Option<String>,
    file_id: Option<String>,
    provider: Option<crate::game::mods_api::ModProvider>,
    summary: Option<String>,
    logo_url: Option<String>,
) -> Result<()> {
    // CORRECCIoN: Usar el mod_id proporcionado en lugar de generar UUID
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
        channel,
        version,
        mod_id,
        mod_name,
        remote_id,
        file_id,
        provider,
        summary,
        logo_url,
    )?;

    Ok(())
}

/// Internal logic to find "Client/" or "Server/" deep within the ZIP and extract it
fn apply_patch_logic(
    zip_path: &Path,
    game_root_dir: &Path,
    backup_dir: &Path,
    patch_dir: &Path,
    channel: String,
    version: String,
    mod_id: String,
    mod_name: String,
    remote_id: Option<String>,
    file_id: Option<String>,
    provider: Option<crate::game::mods_api::ModProvider>,
    summary: Option<String>,
    logo_url: Option<String>,
) -> Result<()> {
    let file = fs::File::open(zip_path)?;
    let mut archive = ZipArchive::new(file)?;

    // 1. Find prefix (where game folders start) and check if hybrid
    let (prefix, is_hybrid) = find_smart_prefix(&mut archive).unwrap_or_default();
    println!("[ZipMods] Base prefix detected: '{}', hybrid: {}", prefix, is_hybrid);

    let mut backups = Vec::new();
    let mut added_files = Vec::new();

    // Phase 1: Identification and Backup
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let raw_name = file.name().to_string(); // Convertir a String para evitar borrowing

        // CRITICAL: Get clean path relative to the game
        let relative_path_str = if prefix.is_empty() {
            raw_name.as_str()
        } else if raw_name.starts_with(&prefix) {
            &raw_name[prefix.len()..]
        } else {
            continue; // Not under the detected prefix
        };

        // STRICT FILTER: Only allow files that start with known game folders
        if !is_game_file(relative_path_str) {
            continue;
        }

        // Ignore empty folders/root
        if relative_path_str.is_empty() || relative_path_str.ends_with('/') {
            continue;
        }

        let target_path = game_root_dir.join(relative_path_str);

        // Backup existing files
        if target_path.exists() {
            let backup_relative = format!("backup_{}", added_files.len());
            let backup_path = backup_dir.join(&backup_relative);
            fs::copy(&target_path, &backup_path)?;
            backups.push((relative_path_str.to_string(), backup_relative));
        }

        // We only extract files, folders are created implicitly above
        if file.is_file() {
            if let Some(p) = target_path.parent() {
                fs::create_dir_all(p)?;
            }
            let mut outfile = fs::File::create(&target_path)?;
            io::copy(&mut file, &mut outfile)?;
            added_files.push(relative_path_str.to_string());
        }
    }

    // Save Manifest
    let manifest = PatchManifest {
        mod_id: mod_id.clone(),
        mod_name: mod_name.clone(),
        install_date: chrono::Utc::now(),
        enabled: true,
        is_hybrid, // Guardamos el estado detectado
        backups,
        added_files,
        remote_id,
        file_id,
        provider,
        summary,
        logo_url,
    };

    let json = serde_json::to_string_pretty(&manifest)?;
    fs::write(patch_dir.join("manifest.json"), json)?;

    // =========================================================
    // LoGICA DE SINCRONIZACIoN DE MODS HiBRIDOS (Gestion Mods/)
    // =========================================================
    
    println!("[ZipMods] DEBUG: Starting hybrid sync logic");
    println!("[ZipMods] DEBUG: is_hybrid = {}", is_hybrid);
    println!("[ZipMods] DEBUG: channel = {}, version = {}", channel, version);
    
    // CORRECCIoN: GamePaths necesita el directorio base de RusTale, no game_root_dir
    // game_root_dir es RusTale/release/latest
    // Necesitamos RusTale
    let rusale_base = game_root_dir.parent()
        .and_then(|p| p.parent())
        .unwrap_or(game_root_dir);
    
    println!("[ZipMods] DEBUG: rusale_base = {:?}", rusale_base);
    
    let game_paths = crate::game::GamePaths::new(rusale_base.to_path_buf());
    let mods_dir = game_paths.mods_dir(&channel, &version);
    let disabled_dir = game_paths.disabled_mods_dir(&channel, &version);
    
    println!("[ZipMods] DEBUG: mods_dir = {:?}", mods_dir);
    println!("[ZipMods] DEBUG: disabled_dir = {:?}", disabled_dir);
    
    // Definimos el nombre estandar para el zip en Mods/.
    // Usamos el mod_name limpio + .zip para consistencia.
    let zip_filename = format!("{}.zip", mod_name);
    let target_active = mods_dir.join(&zip_filename);
    let target_disabled = disabled_dir.join(&zip_filename);
    
    println!("[ZipMods] DEBUG: zip_filename = {}", zip_filename);
    println!("[ZipMods] DEBUG: target_active = {:?}", target_active);
    println!("[ZipMods] DEBUG: target_disabled = {:?}", target_disabled);
    println!("[ZipMods] DEBUG: source zip_path = {:?}", zip_path);

    if is_hybrid {
        // CASO 1: Es hibrido. Debe existir en Mods/ (Activo)
        println!("[ZipMods] Hybrid detected. Syncing to Mods folder: {:?}", target_active);
        
        // Crear carpeta Mods si no existe
        if !mods_dir.exists() { 
            println!("[ZipMods] DEBUG: Creating mods directory");
            let _ = fs::create_dir_all(&mods_dir); 
        } else {
            println!("[ZipMods] DEBUG: Mods directory already exists");
        }

        // 1. Borrar de Disabled por si estaba ahi (para evitar duplicados)
        if target_disabled.exists() { 
            println!("[ZipMods] DEBUG: Removing from disabled mods");
            let _ = fs::remove_file(&target_disabled); 
        }

        // 2. Copiar el source.zip a Mods/NombreMod.zip
        println!("[ZipMods] DEBUG: Verifying source file exists...");
        if !zip_path.exists() {
            println!("[ZipMods] ERROR: Source file does not exist: {:?}", zip_path);
            return Err(anyhow::anyhow!("Source file does not exist: {:?}", zip_path));
        }
        if let Ok(metadata) = fs::metadata(zip_path) {
            println!("[ZipMods] DEBUG: Source file size: {} bytes", metadata.len());
        }
        
        println!("[ZipMods] DEBUG: Attempting to copy from {:?} to {:?}", zip_path, target_active);
        match fs::copy(zip_path, &target_active) {
            Ok(bytes) => {
                println!("[ZipMods] DEBUG: Successfully copied {} bytes", bytes);
                
                // VERIFICACIoN POST-COPIA
                println!("[ZipMods] DEBUG: Verifying file exists after copy...");
                if target_active.exists() {
                    println!("[ZipMods] DEBUG: File exists at target location");
                    if let Ok(metadata) = fs::metadata(&target_active) {
                        println!("[ZipMods] DEBUG: File size: {} bytes", metadata.len());
                        println!("[ZipMods] DEBUG: File permissions: {:?}", metadata.permissions());
                    }
                } else {
                    println!("[ZipMods] DEBUG: ERROR: FILE DOES NOT EXIST AFTER COPY!");
                    println!("[ZipMods] DEBUG: Listing Mods directory contents:");
                    if let Ok(entries) = fs::read_dir(&mods_dir) {
                        for entry in entries.flatten() {
                            println!("[ZipMods] DEBUG:   {:?}", entry.path());
                        }
                    }
                }
            }
            Err(e) => {
                println!("[ZipMods] ERROR: Failed to copy hybrid zip: {}", e);
                return Err(e).with_context(|| format!("Failed to copy hybrid zip to {:?}", target_active))?;
            }
        }
            
    } else {
        // CASO 2: NO es hibrido (Cliente puro). Limpieza.
        // Si el mod se actualizo y DEJo de ser hibrido, hay que borrar el archivo viejo de Mods/
        println!("[ZipMods] DEBUG: Not hybrid, cleaning up any leftovers");

        if target_active.exists() {
            println!("[ZipMods] Patch is pure client. Removing leftover from Mods: {:?}", target_active);
            let _ = fs::remove_file(&target_active);
        }
        if target_disabled.exists() {
            println!("[ZipMods] Patch is pure client. Removing leftover from DisabledMods: {:?}", target_disabled);
            let _ = fs::remove_file(&target_disabled);
        }
    }

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

    // =========================================================
    // LoGICA HYBRID: Mover de Mods/ -> DisabledMods/
    // =========================================================
    if manifest.is_hybrid {
        // Necesitamos reconstruir channel/version para paths
        let (channel, version) = extract_channel_version_from_paths(&core_patches_root);
        
        // CORRECCIoN: Usar directorio base de RusTale
        let rusale_base = game_root_dir.parent()
            .and_then(|p| p.parent())
            .unwrap_or(&game_root_dir);
        
        let game_paths = crate::game::GamePaths::new(rusale_base.to_path_buf());
        let mods_dir = game_paths.mods_dir(&channel, &version);
        let disabled_dir = game_paths.disabled_mods_dir(&channel, &version);
        
        let zip_filename = format!("{}.zip", manifest.mod_name);
        let src = mods_dir.join(&zip_filename);
        let dst = disabled_dir.join(&zip_filename);
        
        if src.exists() {
            if !disabled_dir.exists() { 
                let _ = fs::create_dir_all(&disabled_dir); 
            }
            println!("[ZipMods] Disabling hybrid: moving {:?} -> {:?}", src, dst);
            // Movemos
            let _ = fs::rename(&src, &dst);
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
    channel: String,
    version: String,
    mod_id: &str,
) -> Result<()> {
    let patch_dir = core_patches_root.join(mod_id);
    let manifest_path = patch_dir.join("manifest.json");
    
    if !manifest_path.exists() {
        anyhow::bail!("Manifest missing for {}", mod_id);
    }

    let manifest: PatchManifest = serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
    if manifest.enabled {
        return Ok(());
    }

    // Pre-check optimizado para hibridos
    if manifest.is_hybrid {
        // CORRECCIoN: Usar directorio base de RusTale
        let rusale_base = game_root_dir.parent()
            .and_then(|p| p.parent())
            .unwrap_or(&game_root_dir);
        
        let game_paths = crate::game::GamePaths::new(rusale_base.to_path_buf());
        let disabled_dir = game_paths.disabled_mods_dir(&channel, &version);
        let mods_dir = game_paths.mods_dir(&channel, &version);
        
        let zip_filename = format!("{}.zip", manifest.mod_name);
        let src_disabled = disabled_dir.join(&zip_filename);
        let dst_active = mods_dir.join(&zip_filename);
        
        if src_disabled.exists() {
            if !mods_dir.exists() { 
                let _ = fs::create_dir_all(&mods_dir); 
            }
            println!("[ZipMods] Enabling hybrid: moving {:?} -> {:?}", src_disabled, dst_active);
            let _ = fs::rename(&src_disabled, &dst_active);
        }
    }

    // Clean old backups and re-apply
    let source_zip = patch_dir.join("source.zip");
    let backup_dir = patch_dir.join("backup");

    if backup_dir.exists() {
        fs::remove_dir_all(&backup_dir)?;
    }
    fs::create_dir_all(&backup_dir)?;

    // Call internal logic (Esto validara archivos, regenerara backups y asegurara archivo en Mods)
    apply_patch_logic(
        &source_zip,
        &game_root_dir,
        &backup_dir,
        &patch_dir,
        channel,
        version,
        manifest.mod_id.clone(),
        manifest.mod_name.clone(),
        manifest.remote_id.clone(),
        manifest.file_id.clone(),
        manifest.provider.clone(),
        manifest.summary.clone(),
        manifest.logo_url.clone(),
    )?;

    Ok(())
}

/// Helper para Uninstall: Asegurar limpieza total
pub fn uninstall_patch(
    game_root_dir: PathBuf,
    core_patches_root: PathBuf,
    mod_id: &str,
) -> Result<()> {
    // 1. Deshabilitar primero (Esto ya restaura backups)
    disable_patch(game_root_dir.clone(), core_patches_root.clone(), mod_id)?;
    
    // 2. Limpieza Extra Hibrida (Eliminar de DisabledMods si quedo ahi tras disable)
    // Para hacer esto bien, leemos el manifest una ultima vez antes de borrar la carpeta
    let patch_dir = core_patches_root.join(mod_id);
    let manifest_path = patch_dir.join("manifest.json");
    if manifest_path.exists() {
        if let Ok(content) = fs::read_to_string(&manifest_path) {
            if let Ok(manifest) = serde_json::from_str::<PatchManifest>(&content) {
                if manifest.is_hybrid {
                    let (c, v) = extract_channel_version_from_paths(&core_patches_root);
                    
                    // CORRECCIoN: Usar directorio base de RusTale
                    let rusale_base = game_root_dir.parent()
                        .and_then(|p| p.parent())
                        .unwrap_or(&game_root_dir);
                    
                    let gp = crate::game::GamePaths::new(rusale_base.to_path_buf());
                    let zip_name = format!("{}.zip", manifest.mod_name);
                    
                    let p1 = gp.mods_dir(&c, &v).join(&zip_name);
                    let p2 = gp.disabled_mods_dir(&c, &v).join(&zip_name);
                    
                    if p1.exists() { 
                        let _ = fs::remove_file(p1); 
                    }
                    if p2.exists() { 
                        let _ = fs::remove_file(p2); 
                    }
                    println!("[ZipMods] Cleaned up hybrid files for uninstall: {}", zip_name);
                }
            }
        }
    }

    // 3. Borrar carpeta de patch
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
/// Returns (prefix, is_hybrid) where is_hybrid indicates if both Client and Server content exist
fn find_smart_prefix(archive: &mut ZipArchive<fs::File>) -> Option<(String, bool)> {
    let mut has_client = false;
    let mut has_server = false;
    let mut client_prefix: Option<String> = None;
    
    for i in 0..archive.len() {
        if let Ok(file) = archive.by_index(i) {
            let name = file.name();

            // Priority: Detect deep nested folders for Client
            if !has_client {
                if let Some(idx) = name.find("/Client/") {
                    client_prefix = Some(name[..idx + 1].to_string());
                    has_client = true;
                } else if name.starts_with("Client/") {
                    client_prefix = Some("".to_string());
                    has_client = true;
                }
            }

            // Check for Server content (anywhere in the structure)
            if name.contains("Server/") || name.starts_with("Server/") {
                has_server = true;
            }

            // If we found both, we can stop early
            if has_client && has_server {
                break;
            }
        }
    }

    // Return prefix and hybrid status
    if has_client {
        Some((client_prefix.unwrap_or_default(), has_client && has_server))
    } else {
        None
    }
}

/// Verifies if a ZIP is a valid patch and returns hybrid status
pub fn is_patch_mod(zip_path: &Path) -> (bool, bool) {
    let file = fs::File::open(zip_path).ok();
    if let Some(f) = file {
        if let Ok(mut archive) = ZipArchive::new(f) {
            // A ZIP is a patch if it contains Client/ somewhere
            // Return (is_patch, is_hybrid)
            if let Some((_, is_hybrid)) = find_smart_prefix(&mut archive) {
                return (true, is_hybrid);
            }
        }
    }
    (false, false)
}

/// Extraer canal y version desde las rutas de carpetas
fn extract_channel_version_from_paths(core_patches_root: &Path) -> (String, String) {
    // Intentar extraer desde core_patches_root que suele tener la estructura: .../channel/version/CorePatches
    if let Some(parent) = core_patches_root.parent() {
        if let Some(version_dir) = parent.file_name() {
            if let Some(grandparent) = parent.parent() {
                if let Some(channel) = grandparent.file_name() {
                    return (
                        channel.to_string_lossy().to_string(),
                        version_dir.to_string_lossy().to_string()
                    );
                }
            }
        }
    }
    
    // ultimo fallback
    ("latest".to_string(), "latest".to_string())
}
