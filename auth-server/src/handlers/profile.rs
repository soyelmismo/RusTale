use axum::{extract::{Path, State}, http::StatusCode, response::Json};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{state::ServerState, models::*};

// GET /profile/uuid/{uuid}
pub async fn handle_profile_lookup_uuid(
    Path(uuid_query): Path<String>,
    State(state): State<Arc<Mutex<ServerState>>>,
) -> (StatusCode, Json<serde_json::Value>) {
    let state = state.lock().await;
    println!(">>> [PROFILE LOOKUP] Buscando UUID: {}", uuid_query);

    if uuid_query == state.uuid {
        let resp = ProfileLookupResponse {
            uuid: state.uuid.clone(),
            username: state.username.clone(),
        };
        return (StatusCode::OK, Json(serde_json::to_value(resp).unwrap()));
    }

    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": "Profile not found" })),
    )
}

// GET /profile/username/{username}
pub async fn handle_profile_lookup_username(
    Path(username_query): Path<String>,
    State(state): State<Arc<Mutex<ServerState>>>,
) -> (StatusCode, Json<serde_json::Value>) {
    let state = state.lock().await;
    println!(">>> [PROFILE LOOKUP] Buscando usuario: {}", username_query);

    // En este emulador simple local, solo "existemos" nosotros.
    // Si buscan nuestro nombre (ignorando mayusculas/minusculas), devolvemos nuestro UUID.
    if username_query.to_lowercase() == state.username.to_lowercase() {
        let resp = ProfileLookupResponse {
            uuid: state.uuid.clone(),
            username: state.username.clone(),
        };
        return (StatusCode::OK, Json(serde_json::to_value(resp).unwrap()));
    }

    // Si buscan a otro, 404 Not Found
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": "Profile not found" })),
    )
}
