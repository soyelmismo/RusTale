use super::{GenericFile, GenericMod, ModProvider, ModRepository, SearchResults};
use anyhow::Result;
use async_trait::async_trait;
use rustale_security::get_private_var;
use serde::Deserialize;
use std::sync::atomic::Ordering;

const API_BASE: &str = "https://api.curseforge.com/v1";
const GAME_ID: i32 = 70216;

/// Obtiene las claves de API de CurseForge de forma segura.
/// Las claves están ofuscadas en el binario y se limpian de la RAM tras su uso (Zeroize).
fn get_api_keys() -> Result<Vec<rustale_security::memory::SafeString>> {

    #[cfg(not(test))]
    {
        // Obtenemos la variable ofuscada desde el sistema de seguridad
        let keys_raw = get_private_var("Z_H");

        if keys_raw.is_empty() {
            anyhow::bail!("Z_H not configured in security suite");
        }

        // Dividir por coma para soportar múltiples keys (rotación)
        let keys: Vec<rustale_security::memory::SafeString> = keys_raw
            .split(',')
            .map(|k| k.trim())
            .filter(|k| !k.is_empty())
            .map(|k| rustale_security::memory::SafeString::new(k.to_string()))
            .collect();

        if keys.is_empty() {
            anyhow::bail!("No valid API keys found in obfuscated storage");
        }

        Ok(keys)
    }
    
    #[cfg(test)]
    {
        Ok(vec![rustale_security::memory::SafeString::new(
            "test-api-key".to_string(),
        )])
    }

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
    #[serde(rename = "gameVersions", default)]
    pub game_versions: Vec<String>,
    #[serde(rename = "fileDate")]
    pub file_date: Option<String>,
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

impl From<CfMod> for GenericMod {
    fn from(cf_mod: CfMod) -> Self {
        GenericMod {
            id: cf_mod.id.to_string(),
            name: cf_mod.name,
            summary: cf_mod.summary,
            author: cf_mod
                .authors
                .first()
                .map(|a| a.name.clone())
                .unwrap_or_default(),
            logo_url: cf_mod.logo.map(|l| l.thumbnail_url),
            downloads: cf_mod.download_count as u64,
            website_url: cf_mod.links.website_url,
            provider: ModProvider::CurseForge,
            latest_files: cf_mod.latest_files.into_iter().map(|f| f.into()).collect(),
        }
    }
}

impl From<CfFile> for GenericFile {
    fn from(cf_file: CfFile) -> Self {
        let date = if let Some(d) = cf_file.file_date {
            chrono::DateTime::parse_from_rfc3339(&d)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or(chrono::Utc::now())
        } else {
            chrono::Utc::now()
        };

        GenericFile {
            file_id: cf_file.id.to_string(),
            name: cf_file.file_name.clone(),
            version_name: cf_file.file_name,
            download_url: cf_file.download_url,
            release_date: date,
            game_versions: cf_file.game_versions,
        }
    }
}

pub struct CurseForgeRepository {
    client: rustale_shared::reqwest::Client,
    api_base: String,
}

impl CurseForgeRepository {
    pub fn new() -> Self {
        Self {
            client: rustale_shared::reqwest::Client::new(),
            api_base: API_BASE.to_string(),
        }
    }

    /// Creates a repository with a custom API base URL (for testing)
    pub fn new_with_base(base_url: String) -> Self {
        Self {
            client: rustale_shared::reqwest::Client::new(),
            api_base: base_url,
        }
    }

    /// Creates a repository with a custom reqwest Client
    pub fn new_with_client(client: rustale_shared::reqwest::Client) -> Self {
        Self {
            client,
            api_base: API_BASE.to_string(),
        }
    }

    /// Applica cabeceras de seguridad activas (Honeypots y Anti-Tamper)
    fn apply_security(
        &self,
        mut req: rustale_shared::reqwest::RequestBuilder,
    ) -> rustale_shared::reqwest::RequestBuilder {
        if rustale_security::tamper::IS_COMPROMISED.load(Ordering::Relaxed) {
            req = req.header("X-RusTale-Trace", "78d2f1a90c334b5e");
            req = req.header("User-Agent", "RusTale/2.0 (Suspicious-Activity)");
            req = req.header("Accept-Encoding", "identity");
        }
        req
    }
}

#[async_trait]
impl ModRepository for CurseForgeRepository {
    async fn search(&self, query: &str, index: u32, page_size: u32) -> Result<SearchResults> {
        let api_keys = get_api_keys()?;

        let mut url = format!(
            "{}/mods/search?gameId={}&pageSize={}&index={}&sortField=2&sortOrder=desc",
            self.api_base, GAME_ID, page_size, index
        );

        if !query.is_empty() {
            url.push_str(&format!("&searchFilter={}", urlencoding::encode(query)));
        }

        // Intentar con cada API key disponible
        for (i, api_key) in api_keys.iter().enumerate() {
            let req = self.client.get(&url);
            let resp = self
                .apply_security(req)
                // Usamos la referencia efímera &* para no crear copias String
                .header("x-api-key", &**api_key)
                .header("Accept", "application/json")
                .send()
                .await;

            match resp {
                Ok(response) => {
                    if response.status().is_success() {
                        let body: SearchResponse = response.json().await?;
                        return Ok(SearchResults {
                            mods: body.data.into_iter().map(|m| m.into()).collect(),
                            total_count: body.pagination.total_count,
                        });
                    } else if i == api_keys.len() - 1 {
                        anyhow::bail!("CurseForge API Error: {}", response.status());
                    }
                }
                Err(e) => {
                    if i == api_keys.len() - 1 {
                        anyhow::bail!("CurseForge Request failed: {}", e);
                    }
                }
            }
        }

        anyhow::bail!("No API keys available or all failed")
    }

    async fn get_versions(&self, mod_id: &str) -> Result<Vec<GenericFile>> {
        let api_keys = get_api_keys()?;
        let url = format!("{}/mods/{}/files", self.api_base, mod_id);

        for (i, api_key) in api_keys.iter().enumerate() {
            let req = self.client.get(&url);
            let resp = self
                .apply_security(req)
                .header("x-api-key", &**api_key)
                .header("Accept", "application/json")
                .send()
                .await;

            match resp {
                Ok(response) => {
                    if response.status().is_success() {
                        let body: FilesResponse = response.json().await?;
                        return Ok(body.data.into_iter().map(|f| f.into()).collect());
                    } else if i == api_keys.len() - 1 {
                        anyhow::bail!("CurseForge API Error (Files): {}", response.status());
                    }
                }
                Err(e) => {
                    if i == api_keys.len() - 1 {
                        anyhow::bail!("CurseForge Request failed (Files): {}", e);
                    }
                }
            }
        }

        anyhow::bail!("No API keys available or all failed")
    }
}
