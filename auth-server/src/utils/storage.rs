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
