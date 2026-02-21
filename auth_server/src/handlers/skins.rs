use axum::{extract::{Path, State}, http::StatusCode, response::{IntoResponse, Json}, http::HeaderMap};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{state::ServerState, models::*, utils::*};

// GET /player-skins
pub async fn handle_player_skins_get(
    headers: HeaderMap,
    State(state): State<Arc<Mutex<ServerState>>>,
) -> impl IntoResponse {
    let state = state.lock().await;
    let target_uuid = extract_uuid_from_auth(headers.get("authorization").cloned(), &state.uuid);

    let user_data = get_user_skins_data(&target_uuid, &state.skins);

    Json(PlayerSkinsResponse {
        active_skin: user_data.active_skin,
        max_skins: 10,
        skins: user_data.player_skins,
    })
}

// POST /player-skins
pub async fn handle_player_skins_post(
    headers: HeaderMap,
    State(state): State<Arc<Mutex<ServerState>>>,
    Json(body): Json<PlayerSkinsPostRequest>,
) -> impl IntoResponse {
    let mut state = state.lock().await;
    let target_uuid = extract_uuid_from_auth(headers.get("authorization").cloned(), &state.uuid);

    let mut user_data = get_user_skins_data(&target_uuid, &state.skins);

    let skin_id = Uuid::new_v4().to_string();
    let skin_name = body.name.unwrap_or_else(|| "New Avatar".to_string());
    let skin_data_str = body.skin_data.unwrap_or_default();

    let new_skin = PlayerSkin {
        id: skin_id.clone(),
        name: skin_name,
        skin_data: skin_data_str.clone(),
        created_at: chrono::Utc::now(),
        updated_at: None,
    };

    user_data.player_skins.push(new_skin);
    user_data.active_skin = Some(skin_id.clone());

    // Update legacy field
    if let Ok(parsed) = serde_json::from_str(&skin_data_str) {
        user_data.skin = parsed;
    }

    state.skins.insert(
        target_uuid.clone(),
        serde_json::to_value(&user_data).unwrap_or_default(),
    );
    save_skins_to_disk_simple(&state.skins).await;

    (
        StatusCode::CREATED,
        Json(serde_json::json!({ "skinId": user_data.active_skin })),
    )
}

// PUT /player-skins/active
pub async fn handle_player_skins_set_active(
    headers: HeaderMap,
    State(state): State<Arc<Mutex<ServerState>>>,
    Json(body): Json<PlayerSkinsSetActiveRequest>,
) -> impl IntoResponse {
    let mut state = state.lock().await;
    let target_uuid = extract_uuid_from_auth(headers.get("authorization").cloned(), &state.uuid);

    let mut user_data = get_user_skins_data(&target_uuid, &state.skins);

    if let Some(skin) = user_data.player_skins.iter().find(|s| s.id == body.skin_id) {
        user_data.active_skin = Some(body.skin_id.clone());
        // Update legacy field
        if let Ok(parsed) = serde_json::from_str(&skin.skin_data) {
            user_data.skin = parsed;
        }

        state.skins.insert(
            target_uuid.clone(),
            serde_json::to_value(&user_data).unwrap_or_default(),
        );
        save_skins_to_disk_simple(&state.skins).await;
    }

    StatusCode::NO_CONTENT
}

// PUT /player-skins/{skin_id}
pub async fn handle_player_skins_update(
    Path(skin_id): Path<String>,
    headers: HeaderMap,
    State(state): State<Arc<Mutex<ServerState>>>,
    Json(body): Json<PlayerSkinsPostRequest>,
) -> impl IntoResponse {
    let mut state = state.lock().await;
    let target_uuid = extract_uuid_from_auth(headers.get("authorization").cloned(), &state.uuid);

    let mut user_data = get_user_skins_data(&target_uuid, &state.skins);

    if let Some(skin) = user_data.player_skins.iter_mut().find(|s| s.id == skin_id) {
        if let Some(name) = body.name {
            skin.name = name;
        }
        if let Some(data) = body.skin_data {
            skin.skin_data = data.clone();
            // If this is the active skin, update the legacy field
            if Some(skin_id.clone()) == user_data.active_skin {
                if let Ok(parsed) = serde_json::from_str(&data) {
                    user_data.skin = parsed;
                }
            }
        }
        skin.updated_at = Some(chrono::Utc::now());

        // Set updated skin as active (matches JS implementation)
        user_data.active_skin = Some(skin_id);

        state.skins.insert(
            target_uuid.clone(),
            serde_json::to_value(&user_data).unwrap_or_default(),
        );
        save_skins_to_disk_simple(&state.skins).await;
    }

    StatusCode::NO_CONTENT
}

// DELETE /player-skins/{skin_id}
pub async fn handle_player_skins_delete(
    Path(skin_id): Path<String>,
    headers: HeaderMap,
    State(state): State<Arc<Mutex<ServerState>>>,
) -> impl IntoResponse {
    let mut state = state.lock().await;
    let target_uuid = extract_uuid_from_auth(headers.get("authorization").cloned(), &state.uuid);

    let mut user_data = get_user_skins_data(&target_uuid, &state.skins);

    let initial_len = user_data.player_skins.len();
    user_data.player_skins.retain(|s| s.id != skin_id);

    if user_data.player_skins.len() < initial_len {
        // If we deleted the active skin, pick a new one
        if user_data.active_skin == Some(skin_id.clone()) {
            if let Some(first) = user_data.player_skins.first() {
                user_data.active_skin = Some(first.id.clone());
                if let Ok(parsed) = serde_json::from_str(&first.skin_data) {
                    user_data.skin = parsed;
                }
            } else {
                user_data.active_skin = None;
            }
        }

        state.skins.insert(
            target_uuid.clone(),
            serde_json::to_value(&user_data).unwrap_or_default(),
        );
        save_skins_to_disk_simple(&state.skins).await;
    }

    StatusCode::NO_CONTENT
}
