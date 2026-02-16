use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use anyhow::{Context, Result};
use std::collections::HashMap;

use super::traits::PatchProvider;

const ACCOUNT_DATA_URL: &str = "https://account-data.hytale.com/";
const LAUNCHER_URL: &str = "https://launcher.hytale.com/";
const SESSIONS_URL: &str = "https://sessions.hytale.com/";

#[derive(Debug, Deserialize)]
pub struct VersionFeed {
    pub latest: VersionInfo,
    pub versions: Vec<VersionInfo>,
}

#[derive(Debug, Deserialize)]
pub struct VersionInfo {
    pub version: i32,
    pub name: String,
    pub release_date: String,
    pub changelog: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LauncherData {
    pub download_url: String,
    pub version: i32,
    pub checksum: String,
}

#[derive(Debug, Deserialize)]
pub struct VersionManifest {
    pub version: i32,
    pub files: HashMap<String, FileInfo>,
    pub patches: HashMap<String, PatchInfo>,
}

#[derive(Debug, Deserialize)]
pub struct FileInfo {
    pub url: String,
    pub size: u64,
    pub checksum: String,
}

#[derive(Debug, Deserialize)]
pub struct PatchInfo {
    pub url: String,
    pub signature_url: Option<String>,
    pub size: u64,
    pub checksum: String,
}

#[derive(Debug, Deserialize)]
pub struct SessionNew {
    pub session_id: String,
    pub expires_at: String,
}

#[derive(Debug, Serialize)]
pub struct SessNewRequest {
    pub uuid: String,
}

#[derive(Debug, Deserialize)]
pub struct AccessTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: String,
}

/// Official Hytale API provider
pub struct HytaleProvider {
    client: Client,
    access_tokens: Option<AccessTokens>,
}

impl HytaleProvider {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            access_tokens: None,
        }
    }

    pub fn with_tokens(access_tokens: AccessTokens) -> Self {
        Self {
            client: Client::new(),
            access_tokens: Some(access_tokens),
        }
    }

    async fn create_request(&self, method: &str, url: &str, body: Option<&serde_json::Value>) -> Result<reqwest::Request> {
        let mut req = match method {
            "GET" => self.client.get(url),
            "POST" => self.client.post(url),
            _ => anyhow::bail!("Unsupported HTTP method: {}", method),
        };

        if let Some(body) = body {
            req = req.json(body);
        }

        if let Some(tokens) = &self.access_tokens {
            req = req.header("Authorization", format!("Bearer {}", tokens.access_token));
        }

        req = req.header("Content-Type", "application/json");

        Ok(req.build()?)
    }

    async fn execute_request(&self, req: reqwest::Request) -> Result<reqwest::Response> {
        let resp = self.client.execute(req).await
            .context("Failed to execute request")?;

        if !resp.status().is_success() {
            anyhow::bail!("Request failed with status: {}", resp.status());
        }

        Ok(resp)
    }

    pub async fn get_new_session(&self, uuid: &str) -> Result<SessionNew> {
        let url = format!("{}game-session/new", SESSIONS_URL);
        let body = serde_json::to_value(SessNewRequest {
            uuid: uuid.to_string(),
        })?;

        let req = self.create_request("POST", &url, Some(&body)).await?;
        let resp = self.execute_request(req).await?;

        let session: SessionNew = resp.json().await
            .context("Failed to parse session response")?;

        Ok(session)
    }

    pub async fn get_jre_feed(&self, channel: &str) -> Result<VersionFeed> {
        let url = format!("{}version/{}/jre.json", LAUNCHER_URL, channel);
        let resp = self.client.get(&url).send().await
            .context("Failed to fetch JRE feed")?;

        if !resp.status().is_success() {
            anyhow::bail!("JRE feed request failed with status: {}", resp.status());
        }

        let feed: VersionFeed = resp.json().await
            .context("Failed to parse JRE feed")?;

        Ok(feed)
    }

    pub async fn get_launcher_feed(&self, channel: &str) -> Result<VersionFeed> {
        let url = format!("{}version/{}/launcher.json", LAUNCHER_URL, channel);
        let resp = self.client.get(&url).send().await
            .context("Failed to fetch launcher feed")?;

        if !resp.status().is_success() {
            anyhow::bail!("Launcher feed request failed with status: {}", resp.status());
        }

        let feed: VersionFeed = resp.json().await
            .context("Failed to parse launcher feed")?;

        Ok(feed)
    }

    pub async fn get_launcher_data(&self, architecture: &str, operating_system: &str) -> Result<LauncherData> {
        let url = format!("{}my-account/get-launcher-data", ACCOUNT_DATA_URL);
        
        let mut url_obj = reqwest::Url::parse(&url)?;
        url_obj.query_pairs_mut()
            .append_pair("arch", architecture)
            .append_pair("os", operating_system);

        let req = self.create_request("GET", &url_obj.to_string(), None).await?;
        let resp = self.execute_request(req).await?;

        let data: LauncherData = resp.json().await
            .context("Failed to parse launcher data")?;

        Ok(data)
    }

    pub async fn get_version_manifest(&self, architecture: &str, operating_system: &str, channel: &str, game_version: i32) -> Result<VersionManifest> {
        let url = format!("{}patches/{}/{}/{}/{}", ACCOUNT_DATA_URL, operating_system, architecture, channel, game_version);
        
        let req = self.create_request("GET", &url, None).await?;
        let resp = self.execute_request(req).await?;

        let manifest: VersionManifest = resp.json().await
            .context("Failed to parse version manifest")?;

        Ok(manifest)
    }

    pub async fn check_version_exists(&self, start_version: i32, end_version: i32, architecture: &str, operating_system: &str, channel: &str) -> bool {
        let url = format!("{}patches/{}/{}/{}/{}/{}.pwr", ACCOUNT_DATA_URL, operating_system, architecture, channel, start_version, end_version);
        
        for _ in 0..5 {
            match self.client.head(&url).send().await {
                Ok(resp) => match resp.status().as_u16() {
                    200 => return true,
                    404 => return false,
                    _ => tokio::time::sleep(tokio::time::Duration::from_secs(3)).await,
                },
                Err(_) => return false,
            }
        }
        false
    }

    pub async fn find_latest_version(&self, current: i32, architecture: &str, operating_system: &str, channel: &str) -> i32 {
        let mut current = if current <= 0 { 1 } else { current };
        let mut last_version = current;
        let mut cur_version = current;

        // Check if there are updates since current version
        if self.check_version_exists(0, current + 1, architecture, operating_system, channel).await {
            // Exponential search
            while self.check_version_exists(0, cur_version, architecture, operating_system, channel).await {
                last_version = cur_version;
                cur_version *= 2;
            }

            // Binary search
            while last_version + 1 < cur_version {
                let middle = (cur_version + last_version) / 2;
                if self.check_version_exists(0, middle, architecture, operating_system, channel).await {
                    last_version = middle;
                } else {
                    cur_version = middle;
                }
            }
        }

        last_version
    }
}

#[async_trait]
impl PatchProvider for HytaleProvider {
    fn name(&self) -> &'static str {
        "hytale-official"
    }

    async fn is_available(&self) -> bool {
        // Check if we have access tokens or if basic endpoints are accessible
        if self.access_tokens.is_none() {
            return false;
        }

        match self.client.get(&format!("{}version/release/launcher.json", LAUNCHER_URL)).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    async fn get_latest_version(&self, channel: &str, os: &str, arch: &str) -> Result<i32> {
        let version = self.find_latest_version(0, arch, os, channel).await;
        Ok(version)
    }

    async fn get_available_versions(&self, channel: &str, os: &str, arch: &str) -> Result<Vec<i32>> {
        let latest = self.get_latest_version(channel, os, arch).await?;
        let mut versions = Vec::new();
        
        for v in 0..=latest {
            if self.check_version_exists(0, v, arch, os, channel).await {
                versions.push(v);
            }
        }

        Ok(versions)
    }

    async fn get_patch_url(&self, channel: &str, os: &str, arch: &str, from_version: i32, to_version: i32) -> Result<String> {
        let url = format!("{}patches/{}/{}/{}/{}/{}.pwr", ACCOUNT_DATA_URL, os, arch, channel, from_version, to_version);
        Ok(url)
    }

    async fn get_patch_signature_url(&self, channel: &str, os: &str, arch: &str, from_version: i32, to_version: i32) -> Result<String> {
        let url = format!("{}patches/{}/{}/{}/{}/{}.pwr.sig", ACCOUNT_DATA_URL, os, arch, channel, from_version, to_version);
        Ok(url)
    }

    async fn has_complete_version(&self, channel: &str, os: &str, arch: &str, version: i32) -> Result<bool> {
        let exists = self.check_version_exists(0, version, arch, os, channel).await;
        Ok(exists)
    }

    async fn get_jre_url(&self, os: &str, arch: &str) -> Result<String> {
        let feed = self.get_jre_feed("release").await?;
        // This would need to be implemented based on the actual feed structure
        anyhow::bail!("JRE URL extraction not implemented for official API")
    }

    async fn get_butler_url(&self, os: &str, arch: &str) -> Result<String> {
        anyhow::bail!("Butler downloads not supported by official API")
    }
}

impl Default for HytaleProvider {
    fn default() -> Self {
        Self::new()
    }
}
