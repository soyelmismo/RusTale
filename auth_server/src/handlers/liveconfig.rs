use axum::{
    extract::Query,
    http::{header, HeaderMap},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct ConfigsQuery {
    version: Option<String>,
}

#[derive(Serialize)]
pub struct FlagValue {
    #[serde(rename = "type")]
    flag_type: &'static str,
    value: bool,
}

#[derive(Serialize)]
pub struct LiveConfigFlags {
    enable_discord_integration: FlagValue,
    enable_in_game_discord_link: FlagValue,
    enable_new_server_discovery: FlagValue,
    enable_news_tiles: FlagValue,
    enable_social_layer: FlagValue,
}

impl LiveConfigFlags {
    fn new() -> Self {
        Self {
            enable_discord_integration: FlagValue { flag_type: "boolean", value: true },
            enable_in_game_discord_link: FlagValue { flag_type: "boolean", value: true },
            enable_new_server_discovery: FlagValue { flag_type: "boolean", value: true },
            enable_news_tiles: FlagValue { flag_type: "boolean", value: true },
            enable_social_layer: FlagValue { flag_type: "boolean", value: true },
        }
    }
}

pub async fn handle_configs_get(
    headers: HeaderMap,
    Query(query): Query<ConfigsQuery>,
) -> impl IntoResponse {
    let client_version = query.version.unwrap_or_else(|| "0.5.4".to_string());
    
    let host = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("127.0.0.1:59313");
        
    let protocol = headers
        .get("x-forwarded-proto")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("http");

    let manifest_url = format!("{}://{}/liveconfig/manifest.json", protocol, host);
    let version = format!("9addd45d7f134d23a46d3f87d85d9de4809bf3779e844879a0e98febe10424bb:v={}", client_version);

    Json(serde_json::json!({
        "flags": LiveConfigFlags::new(),
        "manifest_url": manifest_url,
        "version": version,
    }))
}

pub async fn handle_liveconfig_manifest() -> impl IntoResponse {
    Json(serde_json::json!({
      "version": "2026-05-26T15:09:45Z",
      "patchline": "release",
      "platform": {
        "os": "any",
        "arch": "any"
      },
      "configs": {
        "feature-flags": {
          "url": "/v1/release/any/any/feature-flags/209d2849185b9bef0d883bdcf57f52b1d981967e9a1ab15244b25dc96ca6708a.json",
          "hash": "209d2849185b9bef0d883bdcf57f52b1d981967e9a1ab15244b25dc96ca6708a",
          "updated": "2026-05-26T15:09:45Z"
        }
      }
    }))
}

pub async fn handle_feature_flags() -> impl IntoResponse {
    Json(LiveConfigFlags::new())
}

pub async fn handle_news_tiles() -> impl IntoResponse {
    Json(serde_json::json!({
      "tiles": [
        {
          "body": "Link your Hytale and Discord accounts to see all of your friends and invite them to play!",
          "created_at": "2026-05-25T20:18:35.376520016Z",
          "cta_url": "hytale://settings/social",
          "id": "835c79aa-eaa6-4093-b03b-2c94a65ce75d",
          "image_url": "https://live-content.hytale.com/news/9bcc038d19ddfcb3ab73f04a71e6ac239e216a0c3215139da65ede38bea63cf6.png",
          "title": "Hytale x Discord"
        }
      ]
    }))
}
