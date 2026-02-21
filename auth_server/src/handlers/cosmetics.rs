use axum::{extract::State, response::IntoResponse};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{state::ServerState, utils::*};

// GET /my-account/cosmetics
pub async fn handle_cosmetics_inventory_get(
    State(state): State<Arc<Mutex<ServerState>>>,
) -> impl IntoResponse {
    let state = state.lock().await;
    let assets_zip_path = state.game_dir.join("Assets.zip");

    let inventory_json = tokio::task::spawn_blocking(move || read_cosmetic_inventory_from_zip(&assets_zip_path))
        .await
        .unwrap_or_else(|_| "{}".to_string());

    ([("Content-Type", "application/json")], inventory_json)
}

// GET /cosmetics/list
pub async fn handle_cosmetics_list_get(
    State(state): State<Arc<Mutex<ServerState>>>,
) -> impl IntoResponse {
    let state = state.lock().await;
    let assets_zip_path = state.game_dir.join("Assets.zip");

    let list_json = tokio::task::spawn_blocking(move || read_cosmetics_from_zip(&assets_zip_path))
        .await
        .unwrap_or_else(|_| "{}".to_string());

    ([("Content-Type", "application/json")], list_json)
}
