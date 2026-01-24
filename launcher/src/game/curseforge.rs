use anyhow::Result;
use serde::Deserialize;

const API_KEY: &str = "$2a$10$bqk254NMZOWVTzLVJCcxEOmhcyUujKxA5xk.kQCN9q0KNYFJd5b32";
const API_BASE: &str = "https://api.curseforge.com/v1";
const GAME_ID: i32 = 70216; // Hytale ID en CurseForge

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
    let mut url = format!(
        "{}/mods/search?gameId={}&pageSize={}&index={}&sortField=2&sortOrder=desc",
        API_BASE, GAME_ID, page_size, index
    );

    if !query.is_empty() {
        url.push_str(&format!("&searchFilter={}", urlencoding::encode(query)));
    }

    let resp = client
        .get(&url)
        .header("x-api-key", API_KEY)
        .header("Accept", "application/json")
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("API Error: {}", resp.status());
    }

    let body: SearchResponse = resp.json().await?;
    Ok(SearchResult {
        mods: body.data,
        total_count: body.pagination.total_count,
    })
}
