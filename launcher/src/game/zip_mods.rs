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
    pub added_files: Vec<String>, // New files added
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
    paths: &crate::game::GamePaths,
    channel: String, // "latest", "beta", etc.
    version: String, // version number
    mod_id: String,  // ID del mod para nombre de carpeta
    mod_name: String,
    remote_id: Option<String>,
    file_id: Option<String>,
    provider: Option<crate::game::mods_api::ModProvider>,
    summary: Option<String>,
    logo_url: Option<String>,
) -> Result<()> {
    let core_patches_root = paths.core_patches_dir(&channel, &version);
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
        paths,
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
    paths: &crate::game::GamePaths,
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
    // Limpiar el nombre del mod al inicio para usarlo en todo el flujo
    let clean_mod_name = mod_name.chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
        .collect::<String>();
    
    let file = fs::File::open(zip_path)?;
    let mut archive = ZipArchive::new(file)?;

    // 1. Find prefix (where game folders start) and check if hybrid
    let (prefix, is_hybrid) = find_smart_prefix(&mut archive).unwrap_or_default();
    println!(
        "[ZipMods] Base prefix detected: '{}', hybrid: {}",
        prefix, is_hybrid
    );

    let mut backups = Vec::new();
    let mut added_files = Vec::new();
    let game_root_dir = paths.version_dir(&channel, &version);

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

        // Add to added_files list for tracking (even for Server files)
        added_files.push(relative_path_str.to_string());

        // Only extract Client/Shared files to game, Server files are handled by ZIP copy
        if relative_path_str.starts_with("Server/") {
            continue; // Don't extract Server files to game directory
        }

        let target_path = game_root_dir.join(relative_path_str);

        // Backup existing files
        if target_path.exists() {
            let backup_relative = format!("backup_{}", added_files.len() - 1); // Use current index
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

    // Save Manifest con el nombre limpio para consistencia
    let manifest = PatchManifest {
        mod_id: mod_id.clone(),
        mod_name: clean_mod_name.clone(), // Usar el nombre limpio
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
    // LoGICA DE SINCRONIZACIoN DE MODS HiBRIDOS (INTEGRAL A LA INSTALACION)
    // =========================================================
    if is_hybrid {
        println!("[ZipMods] Hybrid detected - performing atomic sync to Mods folder");
        
        // CORRECCIoN: Usar GamePaths pasado como argumento
        let mods_dir = paths.mods_dir(&channel, &version);
        let disabled_dir = paths.disabled_mods_dir(&channel, &version);

        // Definimos el nombre estandar para el zip en Mods/.
        let zip_filename = format!("{}.zip", clean_mod_name);
        let target_active = mods_dir.join(&zip_filename);
        let target_disabled = disabled_dir.join(&zip_filename);

        println!("[ZipMods] DEBUG: mods_dir = {:?}", mods_dir);
        println!("[ZipMods] DEBUG: target_active = {:?}", target_active);
        println!("[ZipMods] DEBUG: source zip_path = {:?}", zip_path);

        // Crear carpeta Mods si no existe
        if !mods_dir.exists() {
            println!("[ZipMods] DEBUG: Creating mods directory");
            let _ = fs::create_dir_all(&mods_dir);
        }

        // 1. Borrar de Disabled por si estaba ahi (para evitar duplicados)
        if target_disabled.exists() {
            println!("[ZipMods] DEBUG: Removing from disabled mods");
            let _ = fs::remove_file(&target_disabled);
        }

        // 2. Copiar el source.zip a Mods/NombreMod.zip - PARTE INTEGRAL DE LA INSTALACION
        println!("[ZipMods] DEBUG: Verifying source file exists...");
        if !zip_path.exists() {
            return Err(anyhow::anyhow!(
                "Source file does not exist: {:?}",
                zip_path
            ));
        }
        if let Ok(metadata) = fs::metadata(zip_path) {
            println!("[ZipMods] DEBUG: Source file size: {} bytes", metadata.len());
        }

        println!(
            "[ZipMods] DEBUG: Attempting to copy from {:?} to {:?}",
            zip_path, target_active
        );
        
        match fs::copy(zip_path, &target_active) {
            Ok(bytes) => {
                println!("[ZipMods] DEBUG: Successfully copied {} bytes", bytes);
                
                // Verificación inmediata
                if target_active.exists() {
                    println!("[ZipMods] DEBUG: File exists at target location");
                    if let Ok(metadata) = fs::metadata(&target_active) {
                        println!("[ZipMods] DEBUG: File size: {} bytes", metadata.len());
                    }
                    
                    // Registrar en mods_manifest.json para consistencia
                    if let Err(e) = register_hybrid_zip_in_manifest(&mods_dir, &clean_mod_name, &manifest) {
                        println!("[ZipMods] Warning: Failed to register hybrid in mods manifest: {}", e);
                        // No fallar la instalación por esto, solo advertir
                    } else {
                        println!("[ZipMods] Successfully registered hybrid in mods manifest");
                    }
                } else {
                    println!("[ZipMods] ERROR: FILE DOES NOT EXIST AFTER COPY!");
                    return Err(anyhow::anyhow!(
                        "Failed to verify copied file at {:?}",
                        target_active
                    ));
                }
            }
            Err(e) => {
                println!("[ZipMods] ERROR: Failed to copy hybrid zip: {}", e);
                return Err(e).with_context(|| {
                    format!("Failed to copy hybrid zip to {:?}", target_active)
                });
            }
        }
        
        println!("[ZipMods] Hybrid sync completed successfully as part of installation");
    } else {
        // CASO 2: NO es hibrido (Cliente puro). Limpieza.
        // Si el mod se actualizo y DEJo de ser hibrido, hay que borrar el archivo viejo de Mods/
        println!("[ZipMods] DEBUG: Not hybrid, cleaning up any leftovers");
        
        let mods_dir = paths.mods_dir(&channel, &version);
        let disabled_dir = paths.disabled_mods_dir(&channel, &version);
        let zip_filename = format!("{}.zip", clean_mod_name);
        let target_active = mods_dir.join(&zip_filename);
        let target_disabled = disabled_dir.join(&zip_filename);

        if target_active.exists() {
            println!(
                "[ZipMods] Patch is pure client. Removing leftover from Mods: {:?}",
                target_active
            );
            let _ = fs::remove_file(&target_active);
        }
        if target_disabled.exists() {
            println!(
                "[ZipMods] Patch is pure client. Removing leftover from DisabledMods: {:?}",
                target_disabled
            );
            let _ = fs::remove_file(&target_disabled);
        }
    }

    println!("[ZipMods] Core patch installation completed successfully");
    Ok(())
}

/// Register hybrid ZIP in mods_manifest.json for consistency
fn register_hybrid_zip_in_manifest(
    mods_dir: &PathBuf,
    mod_name: &str,
    patch_manifest: &PatchManifest,
) -> Result<()> {
    let manifest_path = mods_dir.join("mods_manifest.json");
    
    // Leer manifest existente o crear uno nuevo
    let mut mods = if manifest_path.exists() {
        let content = fs::read_to_string(&manifest_path)?;
        serde_json::from_str::<Vec<serde_json::Value>>(&content)?
    } else {
        Vec::new()
    };
    
    // Crear entrada para el mod híbrido
    let hybrid_entry = serde_json::json!({
        "file_name": format!("{}.zip", mod_name),
        "mod_name": patch_manifest.mod_name,
        "provider": patch_manifest.provider,
        "mod_id": patch_manifest.remote_id,
        "file_id": patch_manifest.file_id,
        "enabled": true,
        "summary": patch_manifest.summary,
        "logo_url": patch_manifest.logo_url,
        "install_date": patch_manifest.install_date,
        "update_available": null,
        "is_hybrid": true
    });
    
    // Agregar al manifest (evitando duplicados)
    if !mods.iter().any(|m| {
        m.get("file_name")
            .and_then(|f| f.as_str())
            .map(|f| f == format!("{}.zip", mod_name))
            .unwrap_or(false)
    }) {
        mods.push(hybrid_entry);
    }
    
    // Escribir manifest actualizado
    let json = serde_json::to_string_pretty(&mods)?;
    fs::write(manifest_path, json)?;
    
    Ok(())
}

/// Critical helper function to filter out garbage
fn is_game_file(path: &str) -> bool {
    // Only accept paths that explicitly start with known game folders
    path.starts_with("Client/") || path.starts_with("Server/") || path.starts_with("Shared/")
}

/// Disable a patch: Restore backups and delete new files. DOES NOT DELETE the source ZIP.
pub fn disable_patch(
    paths: &crate::game::GamePaths,
    channel: String,
    version: String,
    mod_id: &str,
) -> Result<()> {
    let core_patches_root = paths.core_patches_dir(&channel, &version);
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

    let game_root_dir = paths.version_dir(&channel, &version);

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
    // LoGICA PARA MODS: Mover de Mods/ -> DisabledMods/
    // =========================================================
    // Mover a DisabledMods si es híbrido O si tiene contenido Server
    if manifest.is_hybrid || has_server_content_in_manifest(&manifest) {
        let mods_dir = paths.mods_dir(&channel, &version);
        let disabled_dir = paths.disabled_mods_dir(&channel, &version);

        let zip_filename = format!("{}.zip", manifest.mod_name);
        let src = mods_dir.join(&zip_filename);
        let dst = disabled_dir.join(&zip_filename);

        if src.exists() {
            if !disabled_dir.exists() {
                let _ = fs::create_dir_all(&disabled_dir);
            }
            println!("[ZipMods] Disabling mod: moving {:?} -> {:?}", src, dst);
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
    paths: &crate::game::GamePaths,
    channel: String,
    version: String,
    mod_id: &str,
) -> Result<()> {
    let core_patches_root = paths.core_patches_dir(&channel, &version);
    let patch_dir = core_patches_root.join(mod_id);
    let manifest_path = patch_dir.join("manifest.json");

    if !manifest_path.exists() {
        anyhow::bail!("Manifest missing for {}", mod_id);
    }

    let manifest: PatchManifest = serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
    if manifest.enabled {
        return Ok(());
    }

    // Pre-check optimizado para mods (hibridos y server-only)
    if manifest.is_hybrid || has_server_content_in_manifest(&manifest) {
        let disabled_dir = paths.disabled_mods_dir(&channel, &version);
        let mods_dir = paths.mods_dir(&channel, &version);

        let zip_filename = format!("{}.zip", manifest.mod_name);
        let src_disabled = disabled_dir.join(&zip_filename);
        let dst_active = mods_dir.join(&zip_filename);

        if src_disabled.exists() {
            if !mods_dir.exists() {
                let _ = fs::create_dir_all(&mods_dir);
            }
            println!(
                "[ZipMods] Enabling mod: moving {:?} -> {:?}",
                src_disabled, dst_active
            );
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
        paths,
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
    paths: &crate::game::GamePaths,
    channel: String,
    version: String,
    mod_id: &str,
) -> Result<()> {
    let core_patches_root = paths.core_patches_dir(&channel, &version);
    let patch_dir = core_patches_root.join(mod_id);
    let manifest_path = patch_dir.join("manifest.json");
    
    // Leer manifest ANTES de deshabilitar para poder usar la información
    let mut should_cleanup_mods = false;
    let mut mod_name = String::new();
    
    if manifest_path.exists() {
        if let Ok(content) = fs::read_to_string(&manifest_path) {
            if let Ok(manifest) = serde_json::from_str::<PatchManifest>(&content) {
                if manifest.is_hybrid || has_server_content_in_manifest(&manifest) {
                    should_cleanup_mods = true;
                    mod_name = manifest.mod_name.clone();
                }
            }
        }
    }
    
    // 1. Deshabilitar primero (Esto ya restaura backups)
    disable_patch(paths, channel.clone(), version.clone(), mod_id)?;

    // 2. Limpieza Extra de Mods (Eliminar de Mods y DisabledMods)
    if should_cleanup_mods {
        let zip_name = format!("{}.zip", mod_name);

        let p1 = paths.mods_dir(&channel, &version).join(&zip_name);
        let p2 = paths.disabled_mods_dir(&channel, &version).join(&zip_name);

        if p1.exists() {
            let _ = fs::remove_file(p1);
        }
        if p2.exists() {
            let _ = fs::remove_file(p2);
        }
        println!(
            "[ZipMods] Cleaned up mod files for uninstall: {}",
            zip_name
        );
        
        // 3. Remove from mods_manifest.json
        let mods_dir = paths.mods_dir(&channel, &version);
        let mods_manifest_path = mods_dir.join("mods_manifest.json");
        
        if mods_manifest_path.exists() {
            if let Ok(content) = fs::read_to_string(&mods_manifest_path) {
                if let Ok(mut mods) = serde_json::from_str::<Vec<serde_json::Value>>(&content) {
                    // Remove the hybrid mod from mods manifest when uninstalled
                    mods.retain(|m| {
                        m.get("file_name")
                            .and_then(|f| f.as_str())
                            .map(|f| f != zip_name)
                            .unwrap_or(true)
                    });
                    
                    // Write updated manifest
                    let updated_json = serde_json::to_string_pretty(&mods)?;
                    fs::write(mods_manifest_path, updated_json)?;
                    println!("[ZipMods] Removed hybrid mod from mods manifest when uninstalled");
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

/// Helper function to check if manifest has server content
fn has_server_content_in_manifest(manifest: &PatchManifest) -> bool {
    // Check if any added files contain Server/ content
    manifest.added_files.iter().any(|file| file.starts_with("Server/"))
}

/// Verify integrity of all patches and sync with actual file system state
pub fn verify_patch_integrity(
    paths: &crate::game::GamePaths,
    channel: &str,
    version: &str,
) -> Result<Vec<String>> {
    println!("[ZipMods] Integrity check starting for channel={}, version={}", channel, version);
    
    let mut fixed_mods = Vec::new();
    let core_patches_root = paths.core_patches_dir(channel, version);
    
    // Listar Mods directory antes de empezar
    let mods_dir = paths.mods_dir(channel, version);
    if mods_dir.exists() {
        println!("[ZipMods] Integrity: Mods directory contents before check:");
        if let Ok(entries) = fs::read_dir(&mods_dir) {
            for entry in entries.flatten() {
                println!("[ZipMods] Integrity:   {:?}", entry.path());
            }
        }
    } else {
        println!("[ZipMods] Integrity: Mods directory does not exist");
    }
    
    // Verificar integridad de mods_manifest.json
    let mods_manifest_path = mods_dir.join("mods_manifest.json");
    if mods_manifest_path.exists() {
        if let Ok(content) = fs::read_to_string(&mods_manifest_path) {
            if let Ok(manifests) = serde_json::from_str::<Vec<serde_json::Value>>(&content) {
                let mut manifests_to_keep = Vec::new();
                
                for manifest in &manifests {
                    if let Some(file_name) = manifest.get("file_name").and_then(|v| v.as_str()) {
                        let file_path = mods_dir.join(file_name);
                        
                        if file_path.exists() {
                            // El archivo existe, mantener en el manifest
                            manifests_to_keep.push(manifest.clone());
                        } else {
                            // El archivo no existe, remover del manifest
                            if let Some(mod_name) = manifest.get("mod_name").and_then(|v| v.as_str()) {
                                println!("[ZipMods] Integrity: Removing missing mod from manifest: {} ({})", mod_name, file_name);
                                fixed_mods.push(format!("{}: Removed from manifest (file missing)", mod_name));
                            }
                        }
                    }
                }
                
                // Escribir el manifest actualizado si hubo cambios
                if manifests_to_keep.len() != manifests.len() {
                    println!("[ZipMods] Integrity: Updating mods_manifest.json, removed {} entries", manifests.len() - manifests_to_keep.len());
                    let updated_json = serde_json::to_string_pretty(&manifests_to_keep)?;
                    fs::write(&mods_manifest_path, updated_json)?;
                }
            }
        }
    }
    
    if !core_patches_root.exists() {
        return Ok(fixed_mods);
    }

    for entry in fs::read_dir(&core_patches_root)? {
        let entry = entry?;
        if entry.path().is_dir() {
            let patch_dir = entry.path();
            let manifest_path = patch_dir.join("manifest.json");
            
            if manifest_path.exists() {
                if let Ok(content) = fs::read_to_string(&manifest_path) {
                    if let Ok(manifest) = serde_json::from_str::<PatchManifest>(&content) {
                        let mod_id = patch_dir.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown");

                        // Check if this patch should have a ZIP in Mods/DisabledMods
                        let should_have_zip = manifest.is_hybrid || has_server_content_in_manifest(&manifest);
                        
                        if should_have_zip {
                            let mods_dir = paths.mods_dir(channel, version);
                            let disabled_dir = paths.disabled_mods_dir(channel, version);
                            let zip_filename = format!("{}.zip", manifest.mod_name);
                            
                            let zip_in_mods = mods_dir.join(&zip_filename).exists();
                            let zip_in_disabled = disabled_dir.join(&zip_filename).exists();
                            
                            // Verificar también el source.zip en corepatches
                            let source_zip = core_patches_root.join(mod_id).join("source.zip");
                            let source_exists = source_zip.exists();
                            
                            println!("[ZipMods] Integrity: {} - zip_in_mods: {}, zip_in_disabled: {}, source_exists: {}", 
                                manifest.mod_name, zip_in_mods, zip_in_disabled, source_exists);
                            
                            // Si el ZIP no existe en ninguna ubicación PERO el source.zip sí existe
                            if !zip_in_mods && !zip_in_disabled && source_exists {
                                // Restaurar el ZIP desde source.zip
                                println!("[ZipMods] Integrity: Restoring missing ZIP from source.zip for {}", manifest.mod_name);
                                fixed_mods.push(format!("{}: Restored ZIP from source.zip", manifest.mod_name));
                                
                                let target = mods_dir.join(&zip_filename);
                                if !mods_dir.exists() {
                                    let _ = fs::create_dir_all(&mods_dir);
                                }
                                
                                match fs::copy(&source_zip, &target) {
                                    Ok(_) => {
                                        println!("[ZipMods] Integrity: Successfully restored ZIP for {}", manifest.mod_name);
                                    }
                                    Err(e) => {
                                        println!("[ZipMods] Integrity: Failed to restore ZIP for {}: {}", manifest.mod_name, e);
                                        // Si falla la restauración, entonces sí eliminar del index
                                        let patch_dir = core_patches_root.join(mod_id);
                                        if patch_dir.exists() {
                                            let _ = fs::remove_dir_all(patch_dir);
                                        }
                                    }
                                }
                            }
                            // Si el ZIP no existe en ninguna ubicación Y tampoco el source.zip
                            else if !zip_in_mods && !zip_in_disabled && !source_exists {
                                // El usuario eliminó completamente el mod - eliminar del index
                                fixed_mods.push(format!("{}: Removed from index (completely missing)", manifest.mod_name));
                                
                                let patch_dir = core_patches_root.join(mod_id);
                                if patch_dir.exists() {
                                    let _ = fs::remove_dir_all(patch_dir);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    
    Ok(fixed_mods)
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

    // Return prefix and hybrid status (hybrid if has both client AND server folders)
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

