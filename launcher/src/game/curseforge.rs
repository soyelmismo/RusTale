use anyhow::Result;
use serde::Deserialize;
use std::env;

const API_BASE: &str = "https://api.curseforge.com/v1";
const GAME_ID: i32 = 70216;

fn get_api_keys() -> Result<Vec<String>> {
    // Cargar variables de entorno desde archivo .env si existe (solo en debug)
    #[cfg(debug_assertions)]
    dotenv::dotenv().ok();

    let keys_str = if let Ok(key) = env::var("CURSEFORGE_API_KEY") {
        if !key.trim().is_empty() {
            key
        } else {
            return Ok(vec![]);
        }
    } else if let Some(key) = option_env!("CURSEFORGE_API_KEY") {
        if !key.trim().is_empty() {
            key.to_string()
        } else {
            return Ok(vec![]);
        }
    } else {
        anyhow::bail!("CURSEFORGE_API_KEY not configured");
    };

    // Split by comma and trim whitespace
    let keys: Vec<String> = keys_str
        .split(',')
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .collect();

    if keys.is_empty() {
        anyhow::bail!("No valid CURSEFORGE_API_KEY found");
    }

    Ok(keys)
}

#[derive(Debug, Clone, Deserialize)]
pub struct CfMod {
    pub id: i32,
    pub name: String,
    pub summary: String,
    pub authors: Vec<CfAuthor>,
    pub logo: Option<CfImage>,
    #[serde(rename = "downloadCount")]
    pub download_count: f64,
    #[serde(rename = "latestFiles")]
    pub latest_files: Vec<CfFile>,
    pub links: CfLinks,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CfAuthor {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CfImage {
    #[serde(rename = "thumbnailUrl")]
    pub thumbnail_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CfFile {
    pub id: i32,
    #[serde(rename = "fileName")]
    pub file_name: String,
    #[serde(rename = "downloadUrl")]
    pub download_url: Option<String>, // A veces es null y requiere otro call, simplificamos por ahora
}

#[derive(Debug, Clone, Deserialize)]
pub struct CfLinks {
    #[serde(rename = "websiteUrl")]
    pub website_url: String,
}

#[derive(Deserialize)]
struct SearchResponse {
    data: Vec<CfMod>,
    pagination: Pagination,
}

#[derive(Deserialize)]
struct Pagination {
    #[serde(rename = "totalCount")]
    pub total_count: u32,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub mods: Vec<CfMod>,
    pub total_count: u32,
}

pub async fn search_mods(
    client: &reqwest::Client,
    query: &str,
    index: u32,
    page_size: u32,
) -> Result<SearchResult> {
    let api_keys = get_api_keys()?;

    let mut url = format!(
        "{}/mods/search?gameId={}&pageSize={}&index={}&sortField=2&sortOrder=desc",
        API_BASE, GAME_ID, page_size, index
    );

    if !query.is_empty() {
        url.push_str(&format!("&searchFilter={}", urlencoding::encode(query)));
    }

    // Try each API key until one succeeds
    for (i, api_key) in api_keys.iter().enumerate() {
        let resp = client
            .get(&url)
            .header("x-api-key", api_key)
            .header("Accept", "application/json")
            .send()
            .await;

        match resp {
            Ok(response) => {
                if response.status().is_success() {
                    let body: SearchResponse = response.json().await?;
                    return Ok(SearchResult {
                        mods: body.data,
                        total_count: body.pagination.total_count,
                    });
                } else {
                    // If this is the last key, return the error
                    if i == api_keys.len() - 1 {
                        anyhow::bail!("API Error: {}", response.status());
                    }
                    // Otherwise continue to next key
                    continue;
                }
            }
            Err(e) => {
                // If this is the last key, return the error
                if i == api_keys.len() - 1 {
                    anyhow::bail!("Request failed: {}", e);
                }
                // Otherwise continue to next key
                continue;
            }
        }
    }

    anyhow::bail!("No API keys available")
}
