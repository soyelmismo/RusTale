use axum::{extract::{Path, State}, http::StatusCode, response::{IntoResponse, Json}, http::HeaderMap};
use bytes::Bytes;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{state::ServerState, models::*, utils::*};

// GET /my-account/game-profile
pub async fn handle_game_profile(
    headers: HeaderMap,
    State(state): State<Arc<Mutex<ServerState>>>,
) -> impl IntoResponse {
    let state = state.lock().await;
    let target_uuid = extract_uuid_from_auth(headers.get("authorization").cloned(), &state.uuid);

    // 1. Obtener los datos de skins (con migración si es necesario)
    let user_data = get_user_skins_data(&target_uuid, &state.skins);
    let skin_obj = user_data.skin;

    // 2. CONVERTIR A STRING (Crucial para cliente de Hytale)
    let skin_string = serde_json::to_string(&skin_obj).ok();

    let info = AccountInfo {
        uuid: target_uuid.clone(),
        username: state.username.clone(),
        entitlements: vec!["game.base".to_string()],
        created_at: chrono::Utc::now(),
        next_name_change_at: chrono::Utc::now() + chrono::Duration::days(30),
        skin: skin_string,
    };

    let mut response = Json(info).into_response();
    response.headers_mut().insert("Cache-Control", "no-store, no-cache, must-revalidate".parse().unwrap());
    response.headers_mut().insert("Pragma", "no-cache".parse().unwrap());
    response.headers_mut().insert("Expires", "0".parse().unwrap());
    response
}

// PUT /my-account/skin
pub async fn handle_skin_put(
    headers: HeaderMap,
    State(state): State<Arc<Mutex<ServerState>>>,
    body: Bytes,
) -> impl IntoResponse {
    let mut state = state.lock().await;
    let target_uuid = extract_uuid_from_auth(headers.get("authorization").cloned(), &state.uuid);

    if let Ok(json_str) = String::from_utf8(body.to_vec()) {
        if let Ok(serde_json::Value::Object(new_parts)) = serde_json::from_str::<serde_json::Value>(&json_str) {
            println!(">>> [SKIN UPDATE] Received update for {}", target_uuid);

            let mut user_data = get_user_skins_data(&target_uuid, &state.skins);
            let mut current_skin = user_data.skin.clone();

            if let serde_json::Value::Object(ref mut current_map) = current_skin {
                for (k, v) in new_parts {
                    current_map.insert(k, v);
                }
            } else {
                current_skin = serde_json::Value::Object(new_parts);
            }

            // Sync with multi-avatar system
            user_data.skin = current_skin.clone();
            if let Some(ref active_id) = user_data.active_skin {
                if let Some(skin) = user_data.player_skins.iter_mut().find(|s| s.id == *active_id) {
                    skin.skin_data = serde_json::to_string(&current_skin).unwrap_or_default();
                    skin.updated_at = Some(chrono::Utc::now());
                }
            }

            state.skins.insert(target_uuid.clone(), serde_json::to_value(&user_data).unwrap_or_default());
            save_skins_to_disk_simple(&state.skins).await;

            return StatusCode::NO_CONTENT;
        }
    }

    println!("[Server] Invalid skin payload received");
    StatusCode::BAD_REQUEST
}

// GET /my-account/cosmetics - eliminado, ahora está en cosmetics.rs
// GET /cosmetics/list - eliminado, ahora está en cosmetics.rs

// GET /my-account/get-launcher-data
pub async fn handle_launcher_data(
    State(state): State<Arc<Mutex<ServerState>>>,
) -> Json<LauncherData> {
    let state = state.lock().await;

    let data = LauncherData {
        eula_accepted_at: chrono::Utc::now(),
        owner: state.uuid.clone(),
        patchlines: Patchlines {
            pre_release: GameVersionInfo {
                build_version: "1.0.0".to_string(),
                newest: 1,
            },
            release: GameVersionInfo {
                build_version: "1.0.0".to_string(),
                newest: 1,
            },
        },
        profiles: vec![LauncherProfileInfo {
            uuid: state.uuid.clone(),
            username: state.username.clone(),
            entitlements: vec!["game.base".to_string()],
        }],
    };

    Json(data)
}

// GET /my-account/get-profiles
pub async fn handle_get_profiles(
    headers: HeaderMap,
    State(state): State<Arc<Mutex<ServerState>>>,
) -> Json<serde_json::Value> {
    let state = state.lock().await;
    let target_uuid = extract_uuid_from_auth(headers.get("authorization").cloned(), &state.uuid);

    Json(serde_json::json!({
        "profiles": [{
            "uuid": target_uuid,
            "username": state.username.clone(),
            "entitlements": ["game.base"]
        }]
    }))
}

// GET /account-data/skin/{uuid}
pub async fn handle_account_data_skin_get(
    Path(target_uuid): Path<String>,
    State(state): State<Arc<Mutex<ServerState>>>,
) -> Json<serde_json::Value> {
    let state = state.lock().await;
    let user_data = get_user_skins_data(&target_uuid, &state.skins);
    Json(user_data.skin)
}
