use axum::{
    extract::{Query, Path, State},
    response::IntoResponse,
    Json,
};
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::state::ServerState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Clone)]
pub struct SantaleServerResponse {
    pub data: Option<Vec<SantaleServer>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SantaleServer {
    pub hostname: String,
    pub port: u16,
    pub name: String,
    pub description: Option<String>,
    pub short_description: Option<String>,
    pub votes_count: Option<u32>,
    #[serde(default)]
    pub game_modes: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub country_code: Option<String>,
    pub is_f2p: Option<bool>,
    pub id: Option<String>,
    pub source: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ServerListing {
    pub audience: u8,
    pub created_at: Option<String>,
    pub description: String,
    pub favorites: u32,
    pub host: String,
    pub is_favorited: bool,
    pub is_liked: bool,
    pub likes: u32,
    pub name: String,
    pub owner_profile_id: Option<String>,
    pub port: u16,
    pub regions: Vec<u8>,
    pub server_type: u8,
    pub uuid: String,
}

#[derive(Debug, Deserialize)]
pub struct DiscoveryQuery {
    pub sort: Option<String>,
    pub offset: Option<usize>,
}
use sha2::{Sha256, Digest};

const DEFAULT_SOURCE_URL: &str = "https://santale.top/api/all-servers";

fn deterministic_uuid(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let mut hash = hasher.finalize();
    hash[6] = (hash[6] & 0x0f) | 0x50;
    hash[8] = (hash[8] & 0x3f) | 0x80;
    format!("{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        hash[0], hash[1], hash[2], hash[3],
        hash[4], hash[5],
        hash[6], hash[7],
        hash[8], hash[9],
        hash[10], hash[11], hash[12], hash[13], hash[14], hash[15]
    )
}

fn map_server_type(server: &SantaleServer) -> u8 {
    let mut modes = Vec::new();
    for m in &server.game_modes { modes.push(m.to_lowercase()); }
    for t in &server.tags { modes.push(t.to_lowercase()); }
    
    if modes.iter().any(|m| m.contains("minigame")) { return 4; }
    if modes.iter().any(|m| m.contains("pvp")) { return 3; }
    if modes.iter().any(|m| m.contains("roleplay") || m.contains("rpg")) { return 1; }
    0
}

fn map_regions(server: &SantaleServer) -> Vec<u8> {
    let country = server.country_code.as_deref().unwrap_or("").to_uppercase();
    match country.as_str() {
        "US" | "CA" | "MX" => vec![0],
        "BR" | "AR" | "CL" => vec![2],
        "GB" | "IE" | "PL" | "DE" | "FR" | "ES" | "IT" | "NL" => vec![3],
        "TR" | "RU" => vec![5],
        "CN" | "JP" | "KR" | "SG" => vec![8],
        "AU" | "NZ" => vec![9],
        _ => vec![0, 1, 3],
    }
}

pub async fn handle_listings_get(
    State(_state): State<Arc<Mutex<ServerState>>>,
    Query(query): Query<DiscoveryQuery>,
) -> impl IntoResponse {
    let mut url = DEFAULT_SOURCE_URL.to_string();
    url.push_str("?per_page=100&page=1");
    if let Some(sort) = &query.sort {
        if sort == "featured" {
            url.push_str("&sort=votes");
        } else {
            url.push_str("&sort=players");
        }
    } else {
        url.push_str("&sort=players");
    }

    match rustale_shared::HTTP_CLIENT.get(&url).send().await {
        Ok(resp) => {
            if let Ok(json_data) = resp.json::<serde_json::Value>().await {
                let mut source_items = Vec::new();
                
                if let Some(arr) = json_data.as_array() {
                    for item in arr {
                        if let Ok(server) = serde_json::from_value::<SantaleServer>(item.clone()) {
                            source_items.push(server);
                        }
                    }
                } else if let Some(obj) = json_data.as_object() {
                    if let Some(data) = obj.get("data").and_then(|d| d.as_array()) {
                        for item in data {
                            if let Ok(server) = serde_json::from_value::<SantaleServer>(item.clone()) {
                                source_items.push(server);
                            }
                        }
                    }
                }

                let mut listings = Vec::new();
                for server in source_items {
                    let stable_id = format!("{}:{}:{}:{}", 
                        server.source.as_deref().unwrap_or("santale"), 
                        server.id.as_deref().unwrap_or(""), 
                        server.hostname, 
                        server.port);
                    let uuid = deterministic_uuid(&stable_id);
                    let votes = server.votes_count.unwrap_or(0);
                    
                    let desc = if let Some(d) = &server.description {
                        d.clone()
                    } else if let Some(sd) = &server.short_description {
                        sd.clone()
                    } else {
                        "".to_string()
                    };

                    let regions = map_regions(&server);
                    let server_type = map_server_type(&server);

                    listings.push(ServerListing {
                        audience: if server.is_f2p == Some(false) { 1 } else { 0 },
                        created_at: server.created_at,
                        description: desc,
                        favorites: votes,
                        host: server.hostname,
                        is_favorited: false,
                        is_liked: false,
                        likes: votes,
                        name: server.name,
                        owner_profile_id: None,
                        port: server.port,
                        regions,
                        server_type,
                        uuid,
                    });
                }
                
                if let Some(sort) = &query.sort {
                    if sort == "featured" {
                        listings.sort_by(|a, b| (b.favorites + b.likes).cmp(&(a.favorites + a.likes)));
                    } else if sort == "random" {
                        listings.sort_by(|a, b| a.uuid.cmp(&b.uuid));
                    } else {
                        listings.sort_by(|a, b| b.likes.cmp(&a.likes));
                    }
                } else {
                    listings.sort_by(|a, b| b.likes.cmp(&a.likes));
                }
                
                let offset = query.offset.unwrap_or(0);
                let final_listings: Vec<ServerListing> = listings.into_iter().skip(offset).collect();

                return Json(final_listings).into_response();
            }
        }
        Err(e) => {
            eprintln!("[Auth Server] Failed to fetch server list: {}", e);
        }
    }
    
    // Return empty array on error
    Json(Vec::<ServerListing>::new()).into_response()
}

pub async fn handle_interaction_post(
    Path((_uuid, _action)): Path<(String, String)>,
    State(_state): State<Arc<Mutex<ServerState>>>,
) -> impl IntoResponse {
    axum::response::Response::builder()
        .status(204)
        .body(axum::body::Body::empty())
        .unwrap()
}
