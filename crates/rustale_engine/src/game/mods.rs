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
    pub file_url: Option<String>, // Direct download URL
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

/// Delete a JAR mod completely from both mods and disabled_mods directories
/// and update the manifest accordingly
pub async fn delete_mod_completely(
    base_dir: &Path,
    channel: &str,
    version: &str,
    mod_name: &str,
) -> Result<()> {
    let paths = crate::game::GamePaths::new(base_dir.to_path_buf());
    
    // Try to find and delete from mods directory using delete_mod
    let mods_dir = paths.mods_dir(&channel, &version);
    let mod_file = mods_dir.join(mod_name);
    if mod_file.exists() {
        let mod_info = ModInfo {
            name: mod_name.to_string(),
            file_name: mod_name.to_string(),
            enabled: false,
            path: mod_file.clone(),
            size: 0, // Not needed for deletion
            metadata: None,
        };
        delete_mod(&mod_info).await?;
        println!("[Mods] Deleted mod from mods directory: {}", mod_name);
    }
    
    // Try to find and delete from disabled_mods directory using delete_mod
    let disabled_dir = paths.disabled_mods_dir(&channel, &version);
    let disabled_file = disabled_dir.join(mod_name);
    if disabled_file.exists() {
        let mod_info = ModInfo {
            name: mod_name.to_string(),
            file_name: mod_name.to_string(),
            enabled: false,
            path: disabled_file.clone(),
            size: 0, // Not needed for deletion
            metadata: None,
        };
        delete_mod(&mod_info).await?;
        println!("[Mods] Deleted mod from disabled_mods directory: {}", mod_name);
    }
    
    // Update manifest to remove the mod entry
    let mut manifest = load_manifest(base_dir, channel, version).await;
    manifest.retain(|m| m.file_name != mod_name);
    save_manifest(base_dir, channel, version, &manifest).await?;
    
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // === Tests for InstalledModMetadata - JSON contract is critical for disk persistence ===

    #[test]
    fn test_installed_mod_metadata_default_enabled() {
        // Test that enabled defaults to true when not present in JSON
        let json = r#"{
            "file_name": "test.jar",
            "mod_name": "Test",
            "provider": "Modrinth",
            "mod_id": "1",
            "file_id": "2",
            "install_date": "2024-01-01T00:00:00Z"
        }"#;

        let metadata: InstalledModMetadata =
            serde_json::from_str(json).expect("Failed to deserialize");
        assert!(metadata.enabled, "enabled should default to true");
    }

    #[test]
    fn test_installed_mod_metadata_with_update() {
        let metadata = InstalledModMetadata {
            file_name: "updatable.jar".to_string(),
            mod_name: "Updatable Mod".to_string(),
            provider: ModProvider::Modrinth,
            mod_id: "mod-123".to_string(),
            file_id: "file-v1".to_string(),
            enabled: true,
            summary: None,
            logo_url: None,
            install_date: chrono::Utc::now(),
            update_available: Some("file-v2".to_string()),
        };

        let json = serde_json::to_string(&metadata).expect("Failed to serialize");
        assert!(json.contains("file-v2"));

        let decoded: InstalledModMetadata =
            serde_json::from_str(&json).expect("Failed to deserialize");
        assert_eq!(decoded.update_available, Some("file-v2".to_string()));
    }

    // === Tests for ModInstallationRequest ===

    #[test]
    fn test_mod_installation_request() {
        let request = ModInstallationRequest {
            mod_id: "local-uuid-123".to_string(),
            mod_name: "New Mod".to_string(),
            remote_id: Some("remote-456".to_string()),
            file_id: Some("file-789".to_string()),
            file_url: Some("https://example.com/download".to_string()),
            provider: Some(ModProvider::CurseForge),
            summary: Some("Mod description".to_string()),
            logo_url: Some("https://example.com/icon.png".to_string()),
        };

        assert_eq!(request.mod_id, "local-uuid-123");
        assert_eq!(request.mod_name, "New Mod");
        assert!(request.remote_id.is_some());
        assert!(request.provider.is_some());
    }

    #[test]
    fn test_mod_installation_request_minimal() {
        let request = ModInstallationRequest {
            mod_id: "local-mod".to_string(),
            mod_name: "Simple Mod".to_string(),
            remote_id: None,
            file_id: None,
            file_url: None,
            provider: None,
            summary: None,
            logo_url: None,
        };

        assert_eq!(request.mod_id, "local-mod");
        assert!(request.remote_id.is_none());
        assert!(request.provider.is_none());
    }

    // === Tests for manifest operations ===

    #[tokio::test]
    async fn test_save_and_load_manifest() {
        let dir = tempdir().expect("Failed to create temp dir");

        let metadata = vec![InstalledModMetadata {
            file_name: "test.jar".to_string(),
            mod_name: "Test Mod".to_string(),
            provider: ModProvider::Modrinth,
            mod_id: "mod-1".to_string(),
            file_id: "file-1".to_string(),
            enabled: true,
            summary: Some("Test summary".to_string()),
            logo_url: None,
            install_date: chrono::Utc::now(),
            update_available: None,
        }];

        save_manifest(dir.path(), "stable", "latest", &metadata)
            .await
            .expect("Failed to save manifest");

        let loaded = load_manifest(dir.path(), "stable", "latest").await;
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].file_name, "test.jar");
        assert_eq!(loaded[0].mod_name, "Test Mod");
        assert_eq!(loaded[0].provider, ModProvider::Modrinth);
    }

    #[tokio::test]
    async fn test_load_manifest_nonexistent() {
        let dir = tempdir().expect("Failed to create temp dir");

        let loaded = load_manifest(dir.path(), "nonexistent", "version").await;
        assert!(loaded.is_empty());
    }

    #[tokio::test]
    async fn test_save_manifest_creates_directory() {
        let dir = tempdir().expect("Failed to create temp dir");

        // Use a channel that doesn't exist yet
        let metadata = vec![InstalledModMetadata {
            file_name: "new.jar".to_string(),
            mod_name: "New Mod".to_string(),
            provider: ModProvider::CurseForge,
            mod_id: "new-1".to_string(),
            file_id: "new-file-1".to_string(),
            enabled: true,
            summary: None,
            logo_url: None,
            install_date: chrono::Utc::now(),
            update_available: None,
        }];

        // Should create the directory structure automatically
        let result = save_manifest(dir.path(), "new-channel", "v1", &metadata).await;
        assert!(result.is_ok());

        // Verify the manifest was created
        let loaded = load_manifest(dir.path(), "new-channel", "v1").await;
        assert_eq!(loaded.len(), 1);
    }

    // === Tests for ensure_mod_dirs ===

    #[tokio::test]
    async fn test_ensure_mod_dirs_creates_directories() {
        let dir = tempdir().expect("Failed to create temp dir");

        let (mods_dir, disabled_dir) = ensure_mod_dirs(dir.path(), "stable", "latest").await;

        assert!(mods_dir.exists());
        assert!(disabled_dir.exists());
        assert!(mods_dir.ends_with("Mods"));
        assert!(disabled_dir.ends_with("DisabledMods"));
    }

    // === Tests for list_mods ===

    #[tokio::test]
    async fn test_list_mods_empty_directory() {
        let dir = tempdir().expect("Failed to create temp dir");

        let mods = list_mods(dir.path(), "stable", "latest")
            .await
            .expect("Failed to list mods");

        assert!(mods.is_empty());
    }

    #[tokio::test]
    async fn test_list_mods_with_files() {
        let dir = tempdir().expect("Failed to create temp dir");
        let (mods_dir, _) = ensure_mod_dirs(dir.path(), "stable", "latest").await;

        // Create test mod files
        tokio::fs::write(mods_dir.join("mod1.jar"), b"jar content")
            .await
            .expect("Failed to write mod1");
        tokio::fs::write(mods_dir.join("mod2.zip"), b"zip content")
            .await
            .expect("Failed to write mod2");
        // Create a file that should be ignored
        tokio::fs::write(mods_dir.join(".hidden"), b"hidden")
            .await
            .expect("Failed to write hidden");
        tokio::fs::write(mods_dir.join("readme.txt"), b"readme")
            .await
            .expect("Failed to write readme");

        let mods = list_mods(dir.path(), "stable", "latest")
            .await
            .expect("Failed to list mods");

        // Should only find .jar and .zip files, sorted alphabetically
        assert_eq!(mods.len(), 2);
        assert_eq!(mods[0].name, "mod1.jar");
        assert_eq!(mods[1].name, "mod2.zip");
        assert!(mods[0].enabled);
        assert!(mods[1].enabled);
    }

    #[tokio::test]
    async fn test_list_mods_with_disabled() {
        let dir = tempdir().expect("Failed to create temp dir");
        let (mods_dir, disabled_dir) = ensure_mod_dirs(dir.path(), "stable", "latest").await;

        // Create enabled mod
        tokio::fs::write(mods_dir.join("enabled.jar"), b"enabled")
            .await
            .expect("Failed to write enabled");
        // Create disabled mod
        tokio::fs::write(disabled_dir.join("disabled.jar"), b"disabled")
            .await
            .expect("Failed to write disabled");

        let mods = list_mods(dir.path(), "stable", "latest")
            .await
            .expect("Failed to list mods");

        assert_eq!(mods.len(), 2);

        let enabled_mod = mods.iter().find(|m| m.name == "enabled.jar").unwrap();
        assert!(enabled_mod.enabled);

        let disabled_mod = mods.iter().find(|m| m.name == "disabled.jar").unwrap();
        assert!(!disabled_mod.enabled);
    }

    // === Tests for toggle_mod ===

    #[tokio::test]
    async fn test_toggle_mod_disable() {
        let dir = tempdir().expect("Failed to create temp dir");
        let (mods_dir, disabled_dir) = ensure_mod_dirs(dir.path(), "stable", "latest").await;

        // Create enabled mod
        let mod_path = mods_dir.join("toggle_me.jar");
        tokio::fs::write(&mod_path, b"content")
            .await
            .expect("Failed to write mod");

        let mod_info = ModInfo {
            name: "toggle_me.jar".to_string(),
            file_name: "toggle_me.jar".to_string(),
            enabled: true,
            path: mod_path.clone(),
            size: 7,
            metadata: None,
        };

        toggle_mod(dir.path(), "stable", "latest", &mod_info)
            .await
            .expect("Failed to toggle mod");

        // Mod should now be in disabled directory
        assert!(!mod_path.exists());
        assert!(disabled_dir.join("toggle_me.jar").exists());
    }

    #[tokio::test]
    async fn test_toggle_mod_enable() {
        let dir = tempdir().expect("Failed to create temp dir");
        let (mods_dir, disabled_dir) = ensure_mod_dirs(dir.path(), "stable", "latest").await;

        // Create disabled mod
        let mod_path = disabled_dir.join("enable_me.jar");
        tokio::fs::write(&mod_path, b"content")
            .await
            .expect("Failed to write mod");

        let mod_info = ModInfo {
            name: "enable_me.jar".to_string(),
            file_name: "enable_me.jar".to_string(),
            enabled: false,
            path: mod_path.clone(),
            size: 7,
            metadata: None,
        };

        toggle_mod(dir.path(), "stable", "latest", &mod_info)
            .await
            .expect("Failed to toggle mod");

        // Mod should now be in mods directory
        assert!(!mod_path.exists());
        assert!(mods_dir.join("enable_me.jar").exists());
    }

    // === Tests for delete_mod ===

    #[tokio::test]
    async fn test_delete_mod() {
        let dir = tempdir().expect("Failed to create temp dir");

        let mod_path = dir.path().join("delete_me.jar");
        tokio::fs::write(&mod_path, b"content")
            .await
            .expect("Failed to write mod");

        let mod_info = ModInfo {
            name: "delete_me.jar".to_string(),
            file_name: "delete_me.jar".to_string(),
            enabled: true,
            path: mod_path.clone(),
            size: 7,
            metadata: None,
        };

        delete_mod(&mod_info).await.expect("Failed to delete mod");

        assert!(!mod_path.exists());
    }

    #[tokio::test]
    async fn test_delete_mod_completely() {
        let dir = tempdir().expect("Failed to create temp dir");

        // Create mods with manifest
        let (mods_dir, disabled_dir) = ensure_mod_dirs(dir.path(), "stable", "latest").await;

        // Create mod in mods directory
        tokio::fs::write(mods_dir.join("complete.jar"), b"content")
            .await
            .expect("Failed to write mod");

        // Create manifest with the mod
        let metadata = vec![InstalledModMetadata {
            file_name: "complete.jar".to_string(),
            mod_name: "Complete Mod".to_string(),
            provider: ModProvider::CurseForge,
            mod_id: "complete-1".to_string(),
            file_id: "complete-file-1".to_string(),
            enabled: true,
            summary: None,
            logo_url: None,
            install_date: chrono::Utc::now(),
            update_available: None,
        }];
        save_manifest(dir.path(), "stable", "latest", &metadata)
            .await
            .expect("Failed to save manifest");

        // Delete completely
        delete_mod_completely(dir.path(), "stable", "latest", "complete.jar")
            .await
            .expect("Failed to delete completely");

        // Verify file is gone
        assert!(!mods_dir.join("complete.jar").exists());
        assert!(!disabled_dir.join("complete.jar").exists());

        // Verify manifest is updated
        let loaded = load_manifest(dir.path(), "stable", "latest").await;
        assert!(loaded.is_empty());
    }
}
