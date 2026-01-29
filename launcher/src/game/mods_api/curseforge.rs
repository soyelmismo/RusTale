use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use std::env;
use super::{GenericMod, GenericFile, ModRepository, SearchResults, ModProvider};

const API_BASE: &str = "https://api.curseforge.com/v1";
const GAME_ID: i32 = 70216;

fn get_api_keys() -> Result<Vec<String>> {
    // Cargar variables de entorno desde archivo .env si existe (solo en debug)
    #[cfg(debug_assertions)]
    {
        println!("Attempting to load .env file...");
        match dotenv::dotenv() {
            Ok(path) => println!("Loaded .env from: {:?}", path),
            Err(e) => println!("Failed to load .env: {}", e),
        }
    }

    fn clean_key_string(key: &str) -> String {
        let key = key.trim();
        if !key.is_empty() {
            // Remove surrounding quotes if present
            if (key.starts_with('"') && key.ends_with('"')) || (key.starts_with('\'') && key.ends_with('\'')) {
                key[1..key.len()-1].to_string()
            } else {
                key.to_string()
            }
        } else {
            String::new()
        }
    }

    let keys_str = if let Ok(key) = env::var("CURSEFORGE_API_KEY") {
        println!("Found CURSEFORGE_API_KEY in env::var: {}", key);
        let cleaned = clean_key_string(&key);
        if cleaned.is_empty() {
            return Ok(vec![]);
        }
        cleaned
    } else if let Some(key) = option_env!("CURSEFORGE_API_KEY") {
        println!("Found CURSEFORGE_API_KEY in option_env: {}", key);
        let cleaned = clean_key_string(key);
        if cleaned.is_empty() {
            return Ok(vec![]);
        }
        cleaned
    } else {
        println!("CURSEFORGE_API_KEY not found in any environment source");
        anyhow::bail!("CURSEFORGE_API_KEY not configured");
    };

    // Split by comma and trim whitespace
    let keys: Vec<String> = keys_str
        .split(',')
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .collect();

    // Debug: Print the parsed keys
    println!("Parsed {} API keys from environment", keys.len());
    for (i, key) in keys.iter().enumerate() {
        println!("Key {}: {}... (length: {})", i + 1, &key[..8.min(key.len())], key.len());
    }

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
    pub download_url: Option<String>, 
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

#[derive(Deserialize)]
struct FilesResponse {
    data: Vec<CfFile>,
}

// Implementación de conversión de CfMod a GenericMod
impl From<CfMod> for GenericMod {
    fn from(cf_mod: CfMod) -> Self {
        GenericMod {
            id: cf_mod.id.to_string(),
            name: cf_mod.name,
            summary: cf_mod.summary,
            author: cf_mod.authors.first().map(|a| a.name.clone()).unwrap_or_default(),
            logo_url: cf_mod.logo.map(|l| l.thumbnail_url),
            downloads: cf_mod.download_count as u64,
            website_url: cf_mod.links.website_url,
            provider: ModProvider::CurseForge,
            latest_files: cf_mod.latest_files.into_iter().map(|f| f.into()).collect(),
        }
    }
}

// Implementación de conversión de CfFile a GenericFile
impl From<CfFile> for GenericFile {
    fn from(cf_file: CfFile) -> Self {
        GenericFile {
            file_id: cf_file.id.to_string(),
            name: cf_file.file_name.clone(),
            version_name: cf_file.file_name,
            download_url: cf_file.download_url,
            release_date: chrono::Utc::now(), 
            game_versions: vec![],
        }
    }
}

pub struct CurseForgeRepository {
    client: reqwest::Client,
}

impl CurseForgeRepository {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl ModRepository for CurseForgeRepository {
    async fn search(&self, query: &str, index: u32, page_size: u32) -> Result<SearchResults> {
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
            println!("Trying CurseForge API key {}: {}...", i + 1, &api_key[..8.min(api_key.len())]);
            let resp = self.client
                .get(&url)
                .header("x-api-key", api_key)
                .header("Accept", "application/json")
                .send()
                .await;

            match resp {
                Ok(response) => {
                    println!("Response status: {}", response.status());
                    if response.status().is_success() {
                        println!("API key {} worked successfully", i + 1);
                        let body: SearchResponse = response.json().await?;
                        return Ok(SearchResults {
                            mods: body.data.into_iter().map(|m| m.into()).collect(),
                            total_count: body.pagination.total_count,
                        });
                    } else {
                        println!("API key {} failed with status: {}", i + 1, response.status());
                        if i == api_keys.len() - 1 {
                            anyhow::bail!("API Error: {}", response.status());
                        }
                        continue;
                    }
                }
                Err(e) => {
                    println!("Request failed with API key {}: {}", i + 1, e);
                    if i == api_keys.len() - 1 {
                        anyhow::bail!("Request failed: {}", e);
                    }
                    continue;
                }
            }
        }

        anyhow::bail!("No API keys available")
    }

    async fn get_versions(&self, mod_id: &str) -> Result<Vec<GenericFile>> {
        let api_keys = get_api_keys()?;
        let url = format!("{}/mods/{}/files", API_BASE, mod_id);

        for (i, api_key) in api_keys.iter().enumerate() {
            println!("Trying CurseForge API key {} for get_versions: {}...", i + 1, &api_key[..8.min(api_key.len())]);
            let resp = self.client
                .get(&url)
                .header("x-api-key", api_key)
                .header("Accept", "application/json")
                .send()
                .await;

            match resp {
                Ok(response) => {
                    println!("get_versions response status: {}", response.status());
                    if response.status().is_success() {
                        println!("API key {} worked successfully for get_versions", i + 1);
                        let body: FilesResponse = response.json().await?;
                        return Ok(body.data.into_iter().map(|f| f.into()).collect());
                    } else {
                        println!("API key {} failed for get_versions with status: {}", i + 1, response.status());
                        if i == api_keys.len() - 1 {
                            anyhow::bail!("API Error: {}", response.status());
                        }
                        continue;
                    }
                }
                Err(e) => {
                    println!("Request failed with API key {}: {}", i + 1, e);
                    if i == api_keys.len() - 1 {
                        anyhow::bail!("Request failed: {}", e);
                    }
                    continue;
                }
            }
        }

        anyhow::bail!("No API keys available")
    }

    async fn get_latest_compatible(&self, mod_id: &str, _current_file_id: &str) -> Result<Option<GenericFile>> {
        // En un caso real filtrariamos por version del juego compatible. 
        // Como CF ordena files por fecha, asumimos el primero.
        let versions = self.get_versions(mod_id).await?;
        // Tomamos el primero como "ultimo"
        Ok(versions.first().cloned())
    }
}

