use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::{fs, io};
use zip::ZipArchive;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PatchManifest {
    pub mod_id: String,
    pub mod_name: String,
    pub install_date: chrono::DateTime<chrono::Utc>,
    /// Archivos que fueron reemplazados (ruta relativa -> ruta backup relativa)
    pub backups: Vec<(String, String)>,
    /// Archivos nuevos que no existían antes (para borrarlos al desinstalar)
    pub added_files: Vec<String>,
}

/// Escanea el ZIP para encontrar la carpeta base que contiene "Client" o "Server".
/// Retorna el prefijo dentro del ZIP que se debe ignorar.
fn find_zip_root(archive: &mut ZipArchive<fs::File>) -> Option<String> {
    let mut shortest_prefix: Option<String> = None;

    for i in 0..archive.len() {
        if let Ok(file) = archive.by_index(i) {
            let name = file.name();
            // Buscamos patrones como ".../Client/" o ".../Server/" o justo al inicio "Client/"
            if let Some(idx) = name.find("Client/") {
                let prefix = name[..idx].to_string();
                if shortest_prefix
                    .as_ref()
                    .map_or(true, |p| prefix.len() < p.len())
                {
                    shortest_prefix = Some(prefix);
                }
            } else if let Some(idx) = name.find("Server/") {
                let prefix = name[..idx].to_string();
                if shortest_prefix
                    .as_ref()
                    .map_or(true, |p| prefix.len() < p.len())
                {
                    shortest_prefix = Some(prefix);
                }
            }
        }
    }
    shortest_prefix
}

/// Instala un mod .zip parcheando los archivos del juego
pub fn install_patch_mod(
    zip_path: PathBuf,
    game_root_dir: PathBuf,   // .../release/game/latest/
    backup_root_dir: PathBuf, // .../UserData/PatchBackups/
    mod_name: String,
) -> Result<()> {
    let file = fs::File::open(&zip_path).context("Failed to open zip file")?;
    let mut archive = ZipArchive::new(file).context("Failed to read zip archive")?;

    // 1. Detectar raíz
    let prefix = find_zip_root(&mut archive)
        .ok_or_else(|| anyhow::anyhow!("Structure 'Client' or 'Server' not found in zip"))?;

    let mod_id = uuid::Uuid::new_v4().to_string();
    let mod_backup_dir = backup_root_dir.join(&mod_id);
    fs::create_dir_all(&mod_backup_dir).context("Failed to create backup dir")?;

    let mut backups = Vec::new();
    let mut added_files = Vec::new();

    println!(
        "[ZipMod] Installing '{}' (ID: {}) using prefix: '{}'",
        mod_name, mod_id, prefix
    );

    // 2. Fase de Backup y Clasificación
    for i in 0..archive.len() {
        let file = archive.by_index(i)?;
        let full_name = file.name();

        if !full_name.starts_with(&prefix) {
            continue;
        }

        // Eliminar el prefijo para obtener la ruta relativa al juego (ej: "Client/HytaleClient.exe")
        let relative_path_str = &full_name[prefix.len()..];
        if relative_path_str.is_empty() || relative_path_str.ends_with('/') {
            continue; // Es un directorio o la raiz misma
        }

        let target_path = game_root_dir.join(relative_path_str);

        // Si el archivo ya existe, lo respaldamos
        if target_path.exists() {
            let backup_rel_path = relative_path_str; // Usamos la misma estructura en backup
            let backup_full_path = mod_backup_dir.join(backup_rel_path);

            if let Some(parent) = backup_full_path.parent() {
                fs::create_dir_all(parent)?;
            }

            fs::copy(&target_path, &backup_full_path)?;
            backups.push((relative_path_str.to_string(), relative_path_str.to_string()));
        } else {
            added_files.push(relative_path_str.to_string());
        }
    }

    // 3. Fase de Extracción (Sobreescritura)
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let full_name = file.name();

        if !full_name.starts_with(&prefix) {
            continue;
        }

        let relative_path_str = &full_name[prefix.len()..];
        if relative_path_str.is_empty() {
            continue;
        }

        let target_path = game_root_dir.join(relative_path_str);

        if file.is_dir() {
            fs::create_dir_all(&target_path)?;
        } else {
            if let Some(p) = target_path.parent() {
                fs::create_dir_all(p)?;
            }
            let mut outfile = fs::File::create(&target_path)?;
            io::copy(&mut file, &mut outfile)?;
        }
    }

    // 4. Guardar Manifiesto
    let manifest = PatchManifest {
        mod_id,
        mod_name,
        install_date: chrono::Utc::now(),
        backups,
        added_files,
    };

    let manifest_path = mod_backup_dir.join("manifest.json");
    let json = serde_json::to_string_pretty(&manifest)?;
    fs::write(manifest_path, json)?;

    println!("[ZipMod] Installation complete.");
    Ok(())
}

/// Desinstala un mod parcheado restaurando los archivos originales
pub fn uninstall_patch_mod(
    game_root_dir: PathBuf,
    backup_root_dir: PathBuf,
    mod_id: &str,
) -> Result<()> {
    let mod_backup_dir = backup_root_dir.join(mod_id);
    let manifest_path = mod_backup_dir.join("manifest.json");

    if !manifest_path.exists() {
        anyhow::bail!("Mod manifest not found for ID: {}", mod_id);
    }

    let content = fs::read_to_string(&manifest_path)?;
    let manifest: PatchManifest = serde_json::from_str(&content)?;

    println!("[ZipMod] Uninstalling '{}'...", manifest.mod_name);

    // 1. Eliminar archivos que el mod añadió (que no existían antes)
    for added_file in &manifest.added_files {
        let target_path = game_root_dir.join(added_file);
        if target_path.exists() {
            fs::remove_file(&target_path).ok(); // Ignorar error si no se puede borrar
        }
        // Nota: No borramos directorios vacíos recursivamente para simplificar,
        // pero se podría añadir limpieza.
    }

    // 2. Restaurar copias de seguridad
    for (rel_path, backup_rel) in &manifest.backups {
        let backup_source = mod_backup_dir.join(backup_rel);
        let target_path = game_root_dir.join(rel_path);

        if backup_source.exists() {
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)?;
            }
            // Sobreescribir el archivo modificado con el original
            fs::copy(&backup_source, &target_path)?;
        }
    }

    // 3. Eliminar carpeta de backup y manifiesto
    fs::remove_dir_all(&mod_backup_dir).context("Failed to clean up backup directory")?;

    println!("[ZipMod] Uninstallation complete.");
    Ok(())
}

/// Lista los mods parcheados instalados actualmente
pub fn list_installed_patch_mods(backup_root_dir: PathBuf) -> Result<Vec<PatchManifest>> {
    let mut mods = Vec::new();
    if !backup_root_dir.exists() {
        // Si no existe la carpeta, simplemente no hay parches, no es un error.
        return Ok(mods);
    }

    for entry in fs::read_dir(backup_root_dir)? {
        let entry = entry?;
        if entry.path().is_dir() {
            let manifest_path = entry.path().join("manifest.json");
            if manifest_path.exists() {
                let content = fs::read_to_string(manifest_path)?;
                if let Ok(manifest) = serde_json::from_str::<PatchManifest>(&content) {
                    mods.push(manifest);
                }
            }
        }
    }
    // Ordenar: lo más nuevo arriba
    mods.sort_by(|a, b| b.install_date.cmp(&a.install_date));
    Ok(mods)
}

pub fn is_patch_mod(zip_path: &std::path::Path) -> bool {
    let file = fs::File::open(zip_path).ok();
    if let Some(f) = file {
        if let Ok(mut archive) = zip::ZipArchive::new(f) {
            return find_zip_root(&mut archive).is_some();
        }
    }
    false
}
