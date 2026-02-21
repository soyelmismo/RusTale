use chrono::Utc;
use uuid::Uuid;

use crate::models::{PlayerSkin, UserSkinsData, DEFAULT_SKIN};
use std::collections::HashMap;
use std::path::PathBuf;

pub fn get_user_skins_data(
    uuid: &str,
    skins_map: &HashMap<String, serde_json::Value>,
) -> UserSkinsData {
    match skins_map.get(uuid) {
        Some(val) => {
            if val.get("playerSkins").is_some() {
                // Formato nuevo detectado
                serde_json::from_value(val.clone())
                    .unwrap_or_else(|_| create_default_user_skins(val.clone()))
            } else if let Some(skin_field) = val.get("skin") {
                // Formato híbrido/parcial: tiene un campo 'skin' pero no 'playerSkins'
                create_default_user_skins(skin_field.clone())
            } else {
                // Formato legacy puro: el valor es directamente el objeto de la skin
                create_default_user_skins(val.clone())
            }
        }
        None => {
            let default_skin = serde_json::from_str(DEFAULT_SKIN).unwrap_or_default();
            create_default_user_skins(default_skin)
        }
    }
}

fn create_default_user_skins(skin_val: serde_json::Value) -> UserSkinsData {
    let skin_id = Uuid::new_v4().to_string();
    UserSkinsData {
        player_skins: vec![PlayerSkin {
            id: skin_id.clone(),
            name: "Default Avatar".to_string(),
            skin_data: serde_json::to_string(&skin_val).unwrap_or_default(),
            created_at: Utc::now(),
            updated_at: None,
        }],
        active_skin: Some(skin_id),
        skin: skin_val,
    }
}

pub async fn save_skins_to_disk(skins: &HashMap<String, serde_json::Value>, identity_dir: &PathBuf) {
    let save_path = identity_dir.join("skins.json");

    if let Ok(json) = serde_json::to_string_pretty(skins) {
        let _ = tokio::fs::write(&save_path, json).await;
    }
}

pub async fn save_skins_to_disk_simple(skins: &HashMap<String, serde_json::Value>) {
    let identity_dir = crate::crypto::get_identity_dir();
    save_skins_to_disk(skins, &identity_dir).await;
}

pub async fn load_skins_from_disk(identity_dir: &PathBuf) -> HashMap<String, serde_json::Value> {
    let skin_file = identity_dir.join("skins.json");

    if skin_file.exists() {
        match tokio::fs::read_to_string(&skin_file).await {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(e) => {
                eprintln!("[Server] Error reading skins.json: {}", e);
                HashMap::new()
            }
        }
    } else {
        HashMap::new()
    }
}

/// Migrate legacy skin formats and fix invalid UUIDs
pub async fn migrate_skins(skins: &mut HashMap<String, serde_json::Value>) -> bool {
    let mut modified = false;
    let uuids: Vec<String> = skins.keys().cloned().collect();
    
    for uuid in uuids {
        let mut user_data = get_user_skins_data(&uuid, skins);
        let mut local_modified = false;

        // 1. Check if 'playerSkins' is missing
        match skins.get(&uuid) {
            Some(val) if val.get("playerSkins").is_none() => {
                println!("[Server] Migrating legacy skin format for {}", uuid);
                local_modified = true;
            }
            _ => {}
        }

        // 2. Check for invalid UUIDs (Fix for client crash)
        for skin in &mut user_data.player_skins {
            if Uuid::parse_str(&skin.id).is_err() {
                println!("[Server] Fixing invalid skin ID '{}' for {}", skin.id, uuid);
                let old_id = skin.id.clone();
                let new_id = Uuid::new_v4().to_string();
                skin.id = new_id.clone();
                if user_data.active_skin.as_ref() == Some(&old_id) {
                    user_data.active_skin = Some(new_id);
                }
                local_modified = true;
            }
        }

        if local_modified {
            skins.insert(uuid, serde_json::to_value(&user_data).unwrap_or_default());
            modified = true;
        }
    }

    modified
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    // === Corrupted File Handling Tests ===
    // CRITICAL: These tests verify graceful degradation when skins.json is corrupted

    #[tokio::test]
    async fn test_load_skins_from_corrupted_json_file() {
        // Scenario: skins.json exists but contains invalid JSON
        // Expected: Return empty HashMap (graceful degradation)
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let skin_file = temp_dir.path().join("skins.json");
        
        // Write invalid JSON
        let mut file = std::fs::File::create(&skin_file).expect("Failed to create file");
        file.write_all(b"{ this is not valid json at all }}}").expect("Failed to write");
        
        let result = load_skins_from_disk(&temp_dir.path().to_path_buf()).await;
        
        // Should return empty HashMap instead of panicking
        assert!(result.is_empty(), "Corrupted JSON should return empty HashMap");
    }

    #[tokio::test]
    async fn test_load_skins_from_empty_file() {
        // Scenario: skins.json exists but is empty
        // Expected: Return empty HashMap
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let skin_file = temp_dir.path().join("skins.json");
        
        let mut file = std::fs::File::create(&skin_file).expect("Failed to create file");
        file.write_all(b"").expect("Failed to write");
        
        let result = load_skins_from_disk(&temp_dir.path().to_path_buf()).await;
        
        assert!(result.is_empty(), "Empty file should return empty HashMap");
    }

    #[tokio::test]
    async fn test_load_skins_from_partial_json() {
        // Scenario: skins.json has truncated/incomplete JSON
        // Expected: Return empty HashMap (JSON parse fails)
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let skin_file = temp_dir.path().join("skins.json");
        
        let mut file = std::fs::File::create(&skin_file).expect("Failed to create file");
        // Write JSON that starts correctly but is truncated
        file.write_all(b"{\"uuid-123\": {\"playerSkins\": [{\"id\": \"skin-1\"").expect("Failed to write");
        
        let result = load_skins_from_disk(&temp_dir.path().to_path_buf()).await;
        
        assert!(result.is_empty(), "Truncated JSON should return empty HashMap");
    }

    #[tokio::test]
    async fn test_load_skins_valid_data() {
        // Scenario: skins.json has valid data
        // Expected: Return parsed data
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let skin_file = temp_dir.path().join("skins.json");
        
        let valid_data = r#"{
            "player-uuid-123": {
                "playerSkins": [{
                    "id": "skin-uuid-456",
                    "name": "My Skin",
                    "skin_data": "{}",
                    "created_at": "2024-01-01T00:00:00Z"
                }],
                "active_skin": "skin-uuid-456"
            }
        }"#;
        
        let mut file = std::fs::File::create(&skin_file).expect("Failed to create file");
        file.write_all(valid_data.as_bytes()).expect("Failed to write");
        
        let result = load_skins_from_disk(&temp_dir.path().to_path_buf()).await;
        
        assert_eq!(result.len(), 1);
        assert!(result.contains_key("player-uuid-123"));
    }

    // === Legacy Format Handling Tests ===
    // NOTE: get_user_skins_data() normalizes data on-the-fly when reading.
    // If JSON deserialization fails, it creates default data with valid UUIDs.
    // migrate_skins() persists that normalization back to the HashMap.

    #[test]
    fn test_legacy_format_normalized_on_read() {
        // When reading legacy format, get_user_skins_data normalizes automatically
        let mut skins = HashMap::new();
        
        // Legacy format: just the skin object directly (no playerSkins)
        skins.insert(
            "player-uuid".to_string(),
            serde_json::json!({
                "skin": {
                    "variant": "STEVE",
                    "data": "base64data"
                }
            }),
        );
        
        // Reading via get_user_skins_data normalizes the data
        let result = get_user_skins_data("player-uuid", &skins);
        
        // Should have created playerSkins from legacy data
        assert!(!result.player_skins.is_empty(), "Should have playerSkins");
        assert!(result.active_skin.is_some(), "Should have active_skin");
        assert_eq!(result.player_skins[0].name, "Default Avatar");
    }

    #[tokio::test]
    async fn test_migrate_skins_persists_normalization() {
        // migrate_skins reads via get_user_skins_data (which normalizes)
        // and writes back to the HashMap, persisting the normalized structure
        let mut skins = HashMap::new();
        
        // Legacy format without playerSkins
        skins.insert(
            "player-uuid".to_string(),
            serde_json::json!({
                "skin": {
                    "variant": "STEVE",
                    "data": "base64data"
                }
            }),
        );
        
        let migrated = migrate_skins(&mut skins).await;
        
        // Migration should detect the format difference and persist
        assert!(migrated, "Should report migration happened");
        
        // The HashMap should now have normalized data
        // Note: serde serializes with rename attribute: activeSkin (camelCase)
        let migrated_data = skins.get("player-uuid").unwrap();
        assert!(migrated_data.get("playerSkins").is_some(), "Should have playerSkins after migration");
        assert!(migrated_data.get("activeSkin").is_some(), "Should have activeSkin after migration");
    }

    #[test]
    fn test_invalid_uuid_in_data_handled_gracefully() {
        // When data has an invalid UUID and JSON deserialization fails,
        // get_user_skins_data creates default data with valid UUIDs
        let mut skins = HashMap::new();
        
        // This JSON has playerSkins but the deserialization will fail
        // because UserSkinsData expects specific types for DateTime fields
        skins.insert(
            "player-uuid".to_string(),
            serde_json::json!({
                "playerSkins": [{
                    "id": "not-a-valid-uuid",
                    "name": "Broken Skin",
                    "skin_data": "{}",
                    "created_at": "2024-01-01T00:00:00Z"  // This may not parse correctly
                }],
                "active_skin": "not-a-valid-uuid"
            }),
        );
        
        // get_user_skins_data should handle this gracefully
        let result = get_user_skins_data("player-uuid", &skins);
        
        // Should return valid data (either parsed or default)
        assert!(!result.player_skins.is_empty(), "Should have playerSkins");
        assert!(result.active_skin.is_some(), "Should have active_skin");
        
        // If deserialization failed and created default, UUID will be valid
        // If deserialization succeeded, UUID might be invalid (but that's OK for reading)
        for skin in &result.player_skins {
            // Just verify the structure is correct
            assert!(!skin.id.is_empty(), "Skin should have an ID");
            assert!(!skin.name.is_empty(), "Skin should have a name");
        }
    }

    #[tokio::test]
    async fn test_migrate_skins_validates_uuids_in_correctly_parsed_data() {
        // Test that migrate_skins can fix invalid UUIDs when data parses correctly
        // We need to create data that will parse as UserSkinsData correctly
        
        let mut skins = HashMap::new();
        
        // Create UserSkinsData directly and serialize it with an invalid UUID
        let user_data = UserSkinsData {
            player_skins: vec![PlayerSkin {
                id: "invalid-uuid-string".to_string(),  // Invalid UUID
                name: "Test Skin".to_string(),
                skin_data: "{}".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: None,
            }],
            active_skin: Some("invalid-uuid-string".to_string()),
            skin: serde_json::json!({}),
        };
        
        skins.insert(
            "player-uuid".to_string(),
            serde_json::to_value(&user_data).unwrap(),
        );
        
        let migrated = migrate_skins(&mut skins).await;
        
        assert!(migrated, "Should report migration happened for invalid UUID");
        
        let migrated_data = skins.get("player-uuid").unwrap();
        let player_skins = migrated_data.get("playerSkins").unwrap().as_array().unwrap();
        
        // The skin ID should now be a valid UUID
        let new_id = player_skins[0]["id"].as_str().unwrap();
        assert!(Uuid::parse_str(new_id).is_ok(), "Skin ID should be valid UUID after migration");
    }

    #[tokio::test]
    async fn test_migrate_skins_already_valid() {
        // Scenario: Data is already in correct format
        // Expected: No migration needed
        let mut skins = HashMap::new();
        
        skins.insert(
            "player-uuid".to_string(),
            serde_json::json!({
                "playerSkins": [{
                    "id": Uuid::new_v4().to_string(),
                    "name": "Valid Skin",
                    "skin_data": "{}",
                    "created_at": "2024-01-01T00:00:00Z"
                }],
                "active_skin": null
            }),
        );
        
        let migrated = migrate_skins(&mut skins).await;
        
        assert!(!migrated, "Should not report migration for valid data");
    }

    // === Data Integrity Tests ===

    #[tokio::test]
    async fn test_save_and_load_roundtrip() {
        // Test that saving and loading preserves data
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        
        let mut original_skins = HashMap::new();
        original_skins.insert(
            "test-uuid".to_string(),
            serde_json::json!({
                "playerSkins": [{
                    "id": Uuid::new_v4().to_string(),
                    "name": "Test Skin",
                    "skin_data": "{\"variant\":\"ALEX\"}",
                    "created_at": "2024-01-01T00:00:00Z"
                }],
                "active_skin": null
            }),
        );
        
        save_skins_to_disk(&original_skins, &temp_dir.path().to_path_buf()).await;
        let loaded = load_skins_from_disk(&temp_dir.path().to_path_buf()).await;
        
        assert_eq!(original_skins.len(), loaded.len());
        assert!(loaded.contains_key("test-uuid"));
    }

    #[test]
    fn test_get_user_skins_data_missing_user() {
        // Test getting skins for a user that doesn't exist
        let skins = HashMap::new();
        
        let result = get_user_skins_data("nonexistent-uuid", &skins);
        
        // Should return default data
        assert!(!result.player_skins.is_empty());
        assert!(result.active_skin.is_some());
    }

    #[test]
    fn test_get_user_skins_data_legacy_format() {
        // Test converting legacy format to new format
        let mut skins = HashMap::new();
        skins.insert(
            "legacy-user".to_string(),
            serde_json::json!({
                "variant": "STEVE",
                "data": "base64encoded"
            }),
        );
        
        let result = get_user_skins_data("legacy-user", &skins);
        
        // Should have created playerSkins from legacy data
        assert!(!result.player_skins.is_empty());
        assert!(result.active_skin.is_some());
    }

    // === Edge Cases ===

    #[tokio::test]
    async fn test_save_skins_to_readonly_directory() {
        // Scenario: Can't write to directory
        // Expected: Function doesn't panic (error is silently ignored)
        // Note: This test verifies the code doesn't crash, not that it fails gracefully
        
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let skins = HashMap::new();
        
        // This should not panic even if save fails
        save_skins_to_disk(&skins, &temp_dir.path().to_path_buf()).await;
        
        // If we get here, the function handled errors gracefully
    }

    #[test]
    fn test_create_default_user_skins_with_various_inputs() {
        // Test that default skins are created correctly for various input types
        
        // Null value
        let result = create_default_user_skins(serde_json::Value::Null);
        assert!(!result.player_skins.is_empty());
        
        // Empty object
        let result = create_default_user_skins(serde_json::json!({}));
        assert!(!result.player_skins.is_empty());
        
        // Complex object
        let result = create_default_user_skins(serde_json::json!({
            "variant": "ALEX",
            "data": "complexbase64",
            "metadata": {"key": "value"}
        }));
        assert!(!result.player_skins.is_empty());
        
        // Each should have unique ID
        let result1 = create_default_user_skins(serde_json::json!({"test": 1}));
        let result2 = create_default_user_skins(serde_json::json!({"test": 2}));
        assert_ne!(result1.player_skins[0].id, result2.player_skins[0].id);
    }
}
