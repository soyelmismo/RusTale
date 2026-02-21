use axum::{extract::State, http::StatusCode, response::{IntoResponse, Json}};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{state::ServerState, models::*, crypto};

pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
        server: "hytale-rust-emulator".into(),
    })
}

pub async fn jwks() -> Json<crate::crypto::JwkSet> {
    let jwks = crypto::get_host_jwks();
    Json(jwks)
}

pub async fn ok_stub() -> Json<serde_json::Value> {
    Json(json!({ "success": true, "received": true }))
}

pub async fn no_content_stub() -> StatusCode {
    StatusCode::NO_CONTENT
}

pub async fn not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}

// POST /internal/update-path
pub async fn handle_update_path(
    State(state): State<Arc<Mutex<ServerState>>>,
    Json(body): Json<UpdatePathRequest>,
) -> impl IntoResponse {
    let mut state = state.lock().await;
    println!(">>> [INTERNAL] Updating Game Dir to: {}", body.game_dir);

    let new_path = std::path::PathBuf::from(&body.game_dir);
    if new_path.exists() {
        state.game_dir = new_path;
        return StatusCode::OK;
    }

    StatusCode::BAD_REQUEST
}

// POST /internal/update-identity
pub async fn handle_update_identity(
    State(state): State<Arc<Mutex<ServerState>>>,
    Json(body): Json<UpdateIdentityRequest>,
) -> impl IntoResponse {
    let mut state = state.lock().await;
    println!(
        ">>> [INTERNAL] Updating Identity to: {} ({})",
        body.username, body.uuid
    );

    state.username = body.username;
    state.uuid = body.uuid;

    StatusCode::OK
}
