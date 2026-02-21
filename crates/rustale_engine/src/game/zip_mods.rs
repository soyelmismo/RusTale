use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::{fs, io};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use zip::ZipArchive;
use crate::game::mods::ModInstallationRequest;

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
    request: ModInstallationRequest, // De 7 argumentos pasamos a 1
    cancel_token: Option<Arc<AtomicBool>>, // <--- NEW ARGUMENT
) -> Result<()> {
    let core_patches_root = paths.core_patches_dir(&channel, &version);
    // CORRECCIoN: Usar el mod_id proporcionado en lugar de generar UUID
    let patch_dir = core_patches_root.join(&request.mod_id);
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
        request,
        cancel_token, // <--- PASS CANCEL TOKEN
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
    request: ModInstallationRequest,
    cancel_token: Option<Arc<AtomicBool>>, // <--- NEW ARGUMENT
) -> Result<()> {
    // Limpiar el nombre del mod al inicio para usarlo en todo el flujo
    let clean_mod_name = request.mod_name
        .chars()
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
        // NEW: Check cancellation in the heavy loop
        if let Some(token) = &cancel_token {
            if token.load(Ordering::Relaxed) {
                return Err(anyhow::anyhow!("Patch application cancelled by user"));
            }
        }
        
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
        mod_id: request.mod_id.clone(),
        mod_name: clean_mod_name.clone(), // Usar el nombre limpio
        install_date: chrono::Utc::now(),
        enabled: true,
        is_hybrid, // Guardamos el estado detectado
        backups,
        added_files,
        remote_id: request.remote_id.clone(),
        file_id: request.file_id.clone(),
        provider: request.provider.clone(),
        summary: request.summary.clone(),
        logo_url: request.logo_url.clone(),
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
            println!(
                "[ZipMods] DEBUG: Source file size: {} bytes",
                metadata.len()
            );
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
                    if let Err(e) =
                        register_hybrid_zip_in_manifest(&mods_dir, &clean_mod_name, &manifest)
                    {
                        println!(
                            "[ZipMods] Warning: Failed to register hybrid in mods manifest: {}",
                            e
                        );
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
                return Err(e)
                    .with_context(|| format!("Failed to copy hybrid zip to {:?}", target_active));
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
    let request = ModInstallationRequest {
        mod_id: manifest.mod_id.clone(),
        mod_name: manifest.mod_name.clone(),
        remote_id: manifest.remote_id.clone(),
        file_id: manifest.file_id.clone(),
        file_url: None, // No direct URL for local ZIP patches
        provider: manifest.provider.clone(),
        summary: manifest.summary.clone(),
        logo_url: manifest.logo_url.clone(),
    };
    
    apply_patch_logic(
        &source_zip,
        paths,
        &backup_dir,
        &patch_dir,
        channel,
        version,
        request,
        None, // No cancellation token for enable operations (user-initiated)
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
        println!("[ZipMods] Cleaned up mod files for uninstall: {}", zip_name);

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
    manifest
        .added_files
        .iter()
        .any(|file| file.starts_with("Server/"))
}

/// Verify integrity of all patches and sync with actual file system state
pub fn verify_patch_integrity(
    paths: &crate::game::GamePaths,
    channel: &str,
    version: &str,
) -> Result<Vec<String>> {
    println!(
        "[ZipMods] Integrity check starting for channel={}, version={}",
        channel, version
    );

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
                            if let Some(mod_name) =
                                manifest.get("mod_name").and_then(|v| v.as_str())
                            {
                                println!(
                                    "[ZipMods] Integrity: Removing missing mod from manifest: {} ({})",
                                    mod_name, file_name
                                );
                                fixed_mods.push(format!(
                                    "{}: Removed from manifest (file missing)",
                                    mod_name
                                ));
                            }
                        }
                    }
                }

                // Escribir el manifest actualizado si hubo cambios
                if manifests_to_keep.len() != manifests.len() {
                    println!(
                        "[ZipMods] Integrity: Updating mods_manifest.json, removed {} entries",
                        manifests.len() - manifests_to_keep.len()
                    );
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
                        let mod_id = patch_dir
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown");

                        // Check if this patch should have a ZIP in Mods/DisabledMods
                        let should_have_zip =
                            manifest.is_hybrid || has_server_content_in_manifest(&manifest);

                        if should_have_zip {
                            let mods_dir = paths.mods_dir(channel, version);
                            let disabled_dir = paths.disabled_mods_dir(channel, version);
                            let zip_filename = format!("{}.zip", manifest.mod_name);

                            let zip_in_mods = mods_dir.join(&zip_filename).exists();
                            let zip_in_disabled = disabled_dir.join(&zip_filename).exists();

                            // Verificar también el source.zip en corepatches
                            let source_zip = core_patches_root.join(mod_id).join("source.zip");
                            let source_exists = source_zip.exists();

                            println!(
                                "[ZipMods] Integrity: {} - zip_in_mods: {}, zip_in_disabled: {}, source_exists: {}",
                                manifest.mod_name, zip_in_mods, zip_in_disabled, source_exists
                            );

                            // Si el ZIP no existe en ninguna ubicación PERO el source.zip sí existe
                            if !zip_in_mods && !zip_in_disabled && source_exists {
                                // Restaurar el ZIP desde source.zip
                                println!(
                                    "[ZipMods] Integrity: Restoring missing ZIP from source.zip for {}",
                                    manifest.mod_name
                                );
                                fixed_mods.push(format!(
                                    "{}: Restored ZIP from source.zip",
                                    manifest.mod_name
                                ));

                                let target = mods_dir.join(&zip_filename);
                                if !mods_dir.exists() {
                                    let _ = fs::create_dir_all(&mods_dir);
                                }

                                match fs::copy(&source_zip, &target) {
                                    Ok(_) => {
                                        println!(
                                            "[ZipMods] Integrity: Successfully restored ZIP for {}",
                                            manifest.mod_name
                                        );
                                    }
                                    Err(e) => {
                                        println!(
                                            "[ZipMods] Integrity: Failed to restore ZIP for {}: {}",
                                            manifest.mod_name, e
                                        );
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
                                fixed_mods.push(format!(
                                    "{}: Removed from index (completely missing)",
                                    manifest.mod_name
                                ));

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    // === Tests for is_game_file ===
    
    #[test]
    fn test_is_game_file_client() {
        assert!(is_game_file("Client/some_file.txt"));
        assert!(is_game_file("Client/subfolder/file.dat"));
    }

    #[test]
    fn test_is_game_file_server() {
        assert!(is_game_file("Server/server.properties"));
        assert!(is_game_file("Server/config/settings.json"));
    }

    #[test]
    fn test_is_game_file_shared() {
        assert!(is_game_file("Shared/assets.png"));
        assert!(is_game_file("Shared/data/common.dat"));
    }

    #[test]
    fn test_is_game_file_invalid() {
        assert!(!is_game_file("README.txt"));
        assert!(!is_game_file("META-INF/manifest.mf"));
        assert!(!is_game_file("random_folder/file.txt"));
        assert!(!is_game_file("client/")); // lowercase, not valid
    }

    // === Tests for PatchManifest ===

    #[test]
    fn test_patch_manifest_serialization() {
        let manifest = PatchManifest {
            mod_id: "test-mod-123".to_string(),
            mod_name: "Test Mod".to_string(),
            install_date: chrono::Utc::now(),
            enabled: true,
            is_hybrid: true,
            backups: vec![
                ("Client/old_file.txt".to_string(), "backup_0".to_string()),
            ],
            added_files: vec!["Client/new_file.txt".to_string()],
            remote_id: Some("curseforge-12345".to_string()),
            file_id: Some("file-67890".to_string()),
            provider: Some(crate::game::mods_api::ModProvider::CurseForge),
            summary: Some("A test mod".to_string()),
            logo_url: Some("https://example.com/logo.png".to_string()),
        };

        let json = serde_json::to_string_pretty(&manifest).expect("Failed to serialize");
        assert!(json.contains("test-mod-123"));
        assert!(json.contains("Test Mod"));
        assert!(json.contains("is_hybrid"));
        assert!(json.contains("curseforge-12345"));

        let decoded: PatchManifest = serde_json::from_str(&json).expect("Failed to deserialize");
        assert_eq!(decoded.mod_id, manifest.mod_id);
        assert_eq!(decoded.is_hybrid, manifest.is_hybrid);
        assert_eq!(decoded.backups.len(), 1);
    }

    #[test]
    fn test_patch_manifest_minimal() {
        let manifest = PatchManifest {
            mod_id: "minimal".to_string(),
            mod_name: "Minimal Mod".to_string(),
            install_date: chrono::Utc::now(),
            enabled: false,
            is_hybrid: false,
            backups: vec![],
            added_files: vec![],
            remote_id: None,
            file_id: None,
            provider: None,
            summary: None,
            logo_url: None,
        };

        let json = serde_json::to_string(&manifest).expect("Failed to serialize");
        let decoded: PatchManifest = serde_json::from_str(&json).expect("Failed to deserialize");
        
        assert_eq!(decoded.mod_id, "minimal");
        assert!(!decoded.enabled);
        assert!(!decoded.is_hybrid);
        assert!(decoded.backups.is_empty());
        assert!(decoded.added_files.is_empty());
    }

    // === Tests for list_patches ===

    #[test]
    fn test_list_patches_empty_directory() {
        let dir = tempdir().expect("Failed to create temp dir");
        let patches_root = dir.path().to_path_buf();
        
        let result = list_patches(patches_root).expect("Failed to list patches");
        assert!(result.is_empty());
    }

    #[test]
    fn test_list_patches_nonexistent_directory() {
        let result = list_patches(PathBuf::from("/nonexistent/path/that/does/not/exist"));
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_list_patches_with_valid_manifest() {
        let dir = tempdir().expect("Failed to create temp dir");
        let patches_root = dir.path().to_path_buf();
        
        // Create a patch directory with manifest
        let patch_dir = patches_root.join("test-mod-123");
        fs::create_dir_all(&patch_dir).expect("Failed to create patch dir");
        
        let manifest = PatchManifest {
            mod_id: "test-mod-123".to_string(),
            mod_name: "Test Mod".to_string(),
            install_date: chrono::Utc::now(),
            enabled: true,
            is_hybrid: false,
            backups: vec![],
            added_files: vec!["Client/test.txt".to_string()],
            remote_id: None,
            file_id: None,
            provider: None,
            summary: None,
            logo_url: None,
        };
        
        let json = serde_json::to_string_pretty(&manifest).expect("Failed to serialize");
        fs::write(patch_dir.join("manifest.json"), json).expect("Failed to write manifest");
        
        let result = list_patches(patches_root).expect("Failed to list patches");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].mod_id, "test-mod-123");
        assert_eq!(result[0].mod_name, "Test Mod");
    }

    // === Tests for ZIP validation ===

    fn create_test_zip(dir: &std::path::Path, filename: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let zip_path = dir.join(filename);
        let file = fs::File::create(&zip_path).expect("Failed to create zip file");
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        
        for (name, content) in files {
            zip.start_file(*name, options).expect("Failed to start file");
            zip.write_all(content).expect("Failed to write content");
        }
        
        zip.finish().expect("Failed to finish zip");
        zip_path
    }

    #[test]
    fn test_is_patch_mod_valid_client_only() {
        let dir = tempdir().expect("Failed to create temp dir");
        
        let zip_path = create_test_zip(
            dir.path(),
            "client_mod.zip",
            &[
                ("Client/mod_file.txt", b"mod content"),
                ("Client/subfolder/another.txt", b"more content"),
            ],
        );
        
        let (is_patch, is_hybrid) = is_patch_mod(&zip_path);
        assert!(is_patch, "Should detect as valid patch");
        assert!(!is_hybrid, "Should NOT be hybrid (no Server folder)");
    }

    #[test]
    fn test_is_patch_mod_hybrid() {
        let dir = tempdir().expect("Failed to create temp dir");
        
        let zip_path = create_test_zip(
            dir.path(),
            "hybrid_mod.zip",
            &[
                ("Client/client_file.txt", b"client content"),
                ("Server/server_file.txt", b"server content"),
            ],
        );
        
        let (is_patch, is_hybrid) = is_patch_mod(&zip_path);
        assert!(is_patch, "Should detect as valid patch");
        assert!(is_hybrid, "Should BE hybrid (has both Client and Server)");
    }

    #[test]
    fn test_is_patch_mod_server_only() {
        let dir = tempdir().expect("Failed to create temp dir");
        
        let zip_path = create_test_zip(
            dir.path(),
            "server_mod.zip",
            &[
                ("Server/server_file.txt", b"server content"),
            ],
        );
        
        let (is_patch, is_hybrid) = is_patch_mod(&zip_path);
        // Server-only mods are NOT detected as patches by find_smart_prefix
        // because it looks for Client/ first
        // This behavior may need adjustment based on requirements
        assert!(!is_patch, "Server-only should NOT be detected as patch by current logic");
    }

    #[test]
    fn test_is_patch_mod_invalid() {
        let dir = tempdir().expect("Failed to create temp dir");
        
        let zip_path = create_test_zip(
            dir.path(),
            "invalid.zip",
            &[
                ("README.txt", b"readme content"),
                ("random_file.dat", b"random data"),
            ],
        );
        
        let (is_patch, is_hybrid) = is_patch_mod(&zip_path);
        assert!(!is_patch, "Should NOT detect as patch");
        assert!(!is_hybrid);
    }

    #[test]
    fn test_is_patch_mod_nested_structure() {
        let dir = tempdir().expect("Failed to create temp dir");
        
        // Simulate nested structure like: MyMod/Client/file.txt
        let zip_path = create_test_zip(
            dir.path(),
            "nested_mod.zip",
            &[
                ("MyMod/Client/mod_file.txt", b"mod content"),
                ("MyMod/Server/server_file.txt", b"server content"),
            ],
        );
        
        let (is_patch, is_hybrid) = is_patch_mod(&zip_path);
        assert!(is_patch, "Should detect nested Client as patch");
        assert!(is_hybrid, "Should detect hybrid from nested structure");
    }

    #[test]
    fn test_is_patch_mod_nonexistent_file() {
        let (is_patch, is_hybrid) = is_patch_mod(Path::new("/nonexistent/file.zip"));
        assert!(!is_patch);
        assert!(!is_hybrid);
    }

    // === Tests for has_server_content_in_manifest ===

    #[test]
    fn test_has_server_content_true() {
        let manifest = PatchManifest {
            mod_id: "test".to_string(),
            mod_name: "Test".to_string(),
            install_date: chrono::Utc::now(),
            enabled: true,
            is_hybrid: false,
            backups: vec![],
            added_files: vec![
                "Client/file.txt".to_string(),
                "Server/config.properties".to_string(),
            ],
            remote_id: None,
            file_id: None,
            provider: None,
            summary: None,
            logo_url: None,
        };
        
        assert!(has_server_content_in_manifest(&manifest));
    }

    #[test]
    fn test_has_server_content_false() {
        let manifest = PatchManifest {
            mod_id: "test".to_string(),
            mod_name: "Test".to_string(),
            install_date: chrono::Utc::now(),
            enabled: true,
            is_hybrid: false,
            backups: vec![],
            added_files: vec![
                "Client/file.txt".to_string(),
                "Shared/asset.png".to_string(),
            ],
            remote_id: None,
            file_id: None,
            provider: None,
            summary: None,
            logo_url: None,
        };
        
        assert!(!has_server_content_in_manifest(&manifest));
    }

    // === Filesystem Error Handling Tests ===

    #[test]
    fn test_list_patches_corrupted_manifest_json() {
        let dir = tempdir().expect("Failed to create temp dir");
        let patches_root = dir.path().to_path_buf();
        
        // Create a patch directory with corrupted JSON
        let patch_dir = patches_root.join("corrupted-mod");
        fs::create_dir_all(&patch_dir).expect("Failed to create patch dir");
        
        // Write invalid JSON
        fs::write(patch_dir.join("manifest.json"), "{ this is not valid json }")
            .expect("Failed to write corrupted manifest");
        
        // Should not panic, just skip the corrupted entry
        let result = list_patches(patches_root);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_list_patches_empty_manifest_file() {
        let dir = tempdir().expect("Failed to create temp dir");
        let patches_root = dir.path().to_path_buf();
        
        let patch_dir = patches_root.join("empty-manifest");
        fs::create_dir_all(&patch_dir).expect("Failed to create patch dir");
        
        // Write empty file
        fs::write(patch_dir.join("manifest.json"), "")
            .expect("Failed to write empty manifest");
        
        let result = list_patches(patches_root);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_is_patch_mod_corrupted_zip() {
        let dir = tempdir().expect("Failed to create temp dir");
        
        // Create a file that looks like a zip but isn't
        let fake_zip = dir.path().join("fake.zip");
        fs::write(&fake_zip, b"This is not a valid ZIP file content at all")
            .expect("Failed to write fake zip");
        
        let (is_patch, is_hybrid) = is_patch_mod(&fake_zip);
        assert!(!is_patch, "Corrupted ZIP should not be detected as patch");
        assert!(!is_hybrid);
    }

    #[test]
    fn test_is_patch_mod_empty_file() {
        let dir = tempdir().expect("Failed to create temp dir");
        
        // Create an empty file
        let empty_zip = dir.path().join("empty.zip");
        fs::write(&empty_zip, b"").expect("Failed to write empty file");
        
        let (is_patch, is_hybrid) = is_patch_mod(&empty_zip);
        assert!(!is_patch, "Empty file should not be detected as patch");
        assert!(!is_hybrid);
    }

    #[test]
    fn test_is_patch_mod_truncated_zip() {
        let dir = tempdir().expect("Failed to create temp dir");
        
        // Create a valid ZIP first, then truncate it
        let zip_path = create_test_zip(
            dir.path(),
            "truncated.zip",
            &[("Client/file.txt", b"content")],
        );
        
        // Get valid content and truncate it
        let content = fs::read(&zip_path).expect("Failed to read zip");
        let truncated = &content[..content.len() / 2]; // Cut in half
        fs::write(&zip_path, truncated).expect("Failed to write truncated zip");
        
        let (is_patch, is_hybrid) = is_patch_mod(&zip_path);
        assert!(!is_patch, "Truncated ZIP should not be detected as patch");
        assert!(!is_hybrid);
    }

    #[test]
    fn test_patch_manifest_missing_optional_fields() {
        // Test that manifests can be deserialized even with missing optional fields
        let json = r#"{
            "mod_id": "minimal",
            "mod_name": "Minimal Mod",
            "install_date": "2024-01-01T00:00:00Z",
            "enabled": true,
            "is_hybrid": false,
            "backups": [],
            "added_files": []
        }"#;
        
        let manifest: PatchManifest = serde_json::from_str(json).expect("Should deserialize");
        assert_eq!(manifest.mod_id, "minimal");
        assert!(manifest.remote_id.is_none());
        assert!(manifest.file_id.is_none());
        assert!(manifest.provider.is_none());
    }

    #[test]
    fn test_list_patches_handles_directory_without_manifest() {
        let dir = tempdir().expect("Failed to create temp dir");
        let patches_root = dir.path().to_path_buf();
        
        // Create a directory without manifest.json
        let patch_dir = patches_root.join("no-manifest-mod");
        fs::create_dir_all(&patch_dir).expect("Failed to create patch dir");
        fs::write(patch_dir.join("other_file.txt"), "some content")
            .expect("Failed to write file");
        
        // Should not crash, just skip
        let result = list_patches(patches_root).expect("Should succeed");
        assert!(result.is_empty(), "Should skip directories without manifest");
    }

    #[test]
    fn test_is_game_file_edge_cases() {
        // Edge cases for path validation
        assert!(!is_game_file("")); // Empty path
        assert!(!is_game_file("Client")); // Just the folder name, no slash
        // Note: "Client/" returns true because it starts with "Client/"
        // The filtering of directory-only entries happens elsewhere in the code
        assert!(is_game_file("Client/.hidden")); // Hidden file (valid)
        assert!(!is_game_file("client/file.txt")); // Wrong case
        assert!(!is_game_file("CLIENT/file.txt")); // Wrong case
        assert!(!is_game_file("Clients/file.txt")); // Similar prefix but wrong
        assert!(!is_game_file("ClientX/file.txt")); // Prefix with suffix
    }

    #[test]
    fn test_patch_manifest_with_unicode() {
        let manifest = PatchManifest {
            mod_id: "unicode-test".to_string(),
            mod_name: "测试模组 🎮 Тест".to_string(),
            install_date: chrono::Utc::now(),
            enabled: true,
            is_hybrid: false,
            backups: vec![],
            added_files: vec!["Client/файл.txt".to_string()],
            remote_id: None,
            file_id: None,
            provider: None,
            summary: Some("Descripción en español".to_string()),
            logo_url: None,
        };

        let json = serde_json::to_string(&manifest).expect("Should handle Unicode");
        let decoded: PatchManifest = serde_json::from_str(&json).expect("Should parse Unicode");
        
        assert_eq!(decoded.mod_name, "测试模组 🎮 Тест");
        assert_eq!(decoded.summary, Some("Descripción en español".to_string()));
    }

    #[test]
    fn test_patch_manifest_with_large_data() {
        // Test handling of manifests with many entries
        let mut backups = Vec::new();
        let mut added_files = Vec::new();
        
        for i in 0..1000 {
            backups.push((
                format!("Client/file_{}.txt", i),
                format!("backup_{}", i),
            ));
            added_files.push(format!("Client/new_file_{}.txt", i));
        }
        
        let manifest = PatchManifest {
            mod_id: "large-mod".to_string(),
            mod_name: "Large Mod".to_string(),
            install_date: chrono::Utc::now(),
            enabled: true,
            is_hybrid: true,
            backups,
            added_files,
            remote_id: None,
            file_id: None,
            provider: None,
            summary: None,
            logo_url: None,
        };

        let json = serde_json::to_string(&manifest).expect("Should serialize large manifest");
        let decoded: PatchManifest = serde_json::from_str(&json).expect("Should deserialize large manifest");
        
        assert_eq!(decoded.backups.len(), 1000);
        assert_eq!(decoded.added_files.len(), 1000);
    }
}
