use anyhow::Result;
use std::path::PathBuf;
use tokio::fs;

pub async fn repair_installation(
    base_dir: PathBuf,
    channel: String,
    version_str: String,
    progress_callback: impl Fn(f32, &str),
) -> Result<()> {
    let paths = crate::game::GamePaths::new(base_dir.clone());
    let version_root = paths.version_dir(&channel, &version_str);

    // 1. Disable Mods (Move JARs to DisabledMods)
    progress_callback(10.0, "Disabling Mods...");
    let mods_dir = paths.mods_dir(&channel, &version_str);
    let disabled_dir = paths.disabled_mods_dir(&channel, &version_str);

    if !disabled_dir.exists() {
        fs::create_dir_all(&disabled_dir).await?;
    }

    if mods_dir.exists() {
        let mut entries = fs::read_dir(&mods_dir).await?;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.is_file() {
                if let Some(name) = path.file_name() {
                    let dest = disabled_dir.join(name);
                    let _ = fs::rename(path, dest).await;
                }
            }
        }
    }

    // 2. Disable Core Patches (Restore Backups)
    progress_callback(30.0, "Reverting Core Patches...");
    let patches_dir = paths.core_patches_dir(&channel, &version_str);
    if patches_dir.exists() {
        if let Ok(patches) = crate::game::zip_mods::list_patches(patches_dir.clone()) {
            for patch in patches {
                if patch.enabled {
                    let paths_clone = paths.clone();
                    let channel_clone = channel.clone();
                    let version_clone = version_str.clone();
                    let mod_id = patch.mod_id.clone();

                    tokio::task::spawn_blocking(move || {
                        let _ = crate::game::zip_mods::disable_patch(
                            &paths_clone,
                            channel_clone,
                            version_clone,
                            &mod_id,
                        );
                    })
                    .await?;
                }
            }
        }
    }

    // 3. Clean Server JARs
    progress_callback(60.0, "Cleaning Server JARs...");
    let locations = vec![version_root.join("Server"), version_root.clone()];

    for loc in locations {
        if loc.exists() {
            let mut entries = fs::read_dir(&loc).await?;
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().to_string();

                if name.starts_with("HytaleServer.") && name.ends_with(".jar") {
                    let _ = fs::remove_file(entry.path()).await;
                }
            }
        }
    }

    // 4. Restore Original Server JAR
    progress_callback(80.0, "Restoring Original Server...");
    let server_folder = version_root.join("Server");
    if let Err(e) = crate::game::patcher::ensure_vanilla_jar(&server_folder) {
        eprintln!(
            "[Repair] Warning: Failed to restore Server folder JAR: {}",
            e
        );
    }

    // Also check root for cases where Server folder is not used or JAR is at root
    if let Err(e) = crate::game::patcher::ensure_vanilla_jar(&version_root) {
        eprintln!("[Repair] Warning: Failed to restore Root folder JAR: {}", e);
    }

    // Limpiar cache de parches de Aurora (opcional pero recomendado)
    let _ = crate::game::patcher::clean_patches_cache(&|_, _, _, _, _, _| {}).await;

    progress_callback(100.0, "Repair Complete");
    Ok(())
}
