use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use anyhow::{Context, Result};
use std::collections::HashMap;

use super::traits::PatchProvider;
use super::utils::*;

const SHIPOFYARN_API_URL: &str = "https://thecute.cloud/ShipOfYarn/api.php";

#[derive(Debug, Clone, Deserialize)]
pub struct ShipOfYarnAPI {
    pub hytale: HytaleData,
    pub jre: JreData,
    pub butler: ButlerData,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct HytaleData {
    pub release: PlatformData,
    #[serde(rename = "pre-release")]
    pub pre_release: PlatformData,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PlatformData {
    pub linux: PlatformFiles,
    pub windows: PlatformFiles,
    pub mac: PlatformFiles,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PlatformFiles {
    #[serde(flatten)]
    pub files: HashMap<String, String>,
    #[allow(dead_code)]
    pub patch: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JreData {
    pub linux: HashMap<String, String>,
    pub windows: HashMap<String, String>,
    pub mac: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ButlerData {
    pub linux: HashMap<String, String>,
    pub windows: HashMap<String, String>,
    pub mac: HashMap<String, String>,
}

/// ShipOfYarn API provider (fallback API)
pub struct ShipOfYarnProvider {
    client: Client,
    cached_data: Option<ShipOfYarnAPI>,
}

impl ShipOfYarnProvider {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            cached_data: None,
        }
    }

    async fn fetch_data(&self) -> Result<ShipOfYarnAPI> {
        if let Some(cached) = &self.cached_data {
            return Ok(cached.clone());
        }

        let response = self.client
            .get(SHIPOFYARN_API_URL)
            .send()
            .await
            .context("Failed to fetch ShipOfYarn API")?;

        if !response.status().is_success() {
            anyhow::bail!("ShipOfYarn API returned status: {}", response.status());
        }

        let response_text = response.text().await.context("Failed to read response body")?;
        
        let data: ShipOfYarnAPI = serde_json::from_str(&response_text)
            .map_err(|e| {
                anyhow::anyhow!("Failed to decode ShipOfYarn response: {}", e)
            })?;
        
        Ok(data)
    }

    fn get_channel<'a>(&self, data: &'a ShipOfYarnAPI, channel: &str) -> Result<&'a PlatformData> {
        match channel {
            "release" => Ok(&data.hytale.release),
            "pre-release" => Ok(&data.hytale.pre_release),
            _ => anyhow::bail!("Unsupported channel: {}", channel),
        }
    }

    fn get_files_for_current_os(channel_data: &PlatformData) -> Result<&HashMap<String, String>> {
        let os_name = std::env::consts::OS;
        match os_name {
            "linux" => Ok(&channel_data.linux.files),
            "windows" => Ok(&channel_data.windows.files),
            "macos" => Ok(&channel_data.mac.files),
            _ => anyhow::bail!("Unsupported OS: {}", os_name),
        }
    }

            
    fn extract_version_from_filename(filename: &str) -> Option<i32> {
        if let Some((_, to)) = extract_versions_from_filename(filename) {
            Some(to)
        } else {
            None
        }
    }

    async fn get_version_url(&self, channel: &str, os: &str, arch: &str, prev_version: i32, target_version: i32) -> Result<String> {
        let data = self.fetch_data().await?;
        let channel_data = self.get_channel(&data, channel)?;
        let os_data = Self::get_files_for_current_os(channel_data)?;

        // Try different filename patterns
        let possible_filenames = vec![
            // Complete version: "v8-linux-amd64.pwr"
            format!("v{}-{}-{}.pwr", target_version, os, arch),
            // Incremental: "v7~8-linux-amd64.pwr"
            format!("v{}~{}-{}-{}.pwr", prev_version, target_version, os, arch),
        ];

        for filename in possible_filenames {
            if let Some(url) = os_data.get(&filename) {
                return Ok(url.clone());
            }
        }

        anyhow::bail!("Patch {}->{} not found for channel {} on {}-{}", prev_version, target_version, channel, os, arch)
    }
}

#[async_trait]
impl PatchProvider for ShipOfYarnProvider {
    fn name(&self) -> &'static str {
        "shipofyarn"
    }

    async fn is_available(&self) -> bool {
        match self.client.head(SHIPOFYARN_API_URL).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    async fn get_latest_version(&self, channel: &str, os: &str, _arch: &str) -> Result<i32> {
        let data = self.fetch_data().await?;
        let channel_data = self.get_channel(&data, channel)?;
        let os_data = Self::get_files_for_current_os(channel_data)?;

        let mut max_version = 0;
        for filename in os_data.keys() {
            if let Some(version) = Self::extract_version_from_filename(filename) {
                if version > max_version {
                    max_version = version;
                }
            }
        }

        if max_version == 0 {
            anyhow::bail!("No versions found for channel {} on {}", channel, os);
        }

        Ok(max_version)
    }

    async fn get_available_versions(&self, channel: &str, os: &str, _arch: &str) -> Result<Vec<i32>> {
        let data = self.fetch_data().await?;
        let channel_data = self.get_channel(&data, channel)?;
        let os_data = Self::get_files_for_current_os(channel_data)?;

        let mut complete_versions = std::collections::HashSet::new();
        let mut incremental_versions = std::collections::HashSet::new();
        
        // Always include version 0 as base version
        complete_versions.insert(0);
        
        // Extract version numbers from filenames
        for filename in os_data.keys() {
            if let Some((from_ver, to_ver)) = extract_versions_from_filename(filename) {
                if from_ver == 0 {
                    // Complete version: v6-linux-amd64.pwr (extracted as 0->6)
                    complete_versions.insert(to_ver);
                } else {
                    // Incremental: v5~6-linux-amd64.pwr
                    incremental_versions.insert(from_ver);
                    incremental_versions.insert(to_ver);
                }
            }
        }

        if complete_versions.is_empty() && incremental_versions.is_empty() {
            anyhow::bail!("No versions found for channel {} on {}", channel, os);
        }

        // For pre-release channels, we need to include all incremental versions
        // since there are no complete versions available
        let mut result: Vec<i32> = if channel == "pre-release" {
            let mut all_versions = std::collections::HashSet::new();
            all_versions.extend(&complete_versions);
            all_versions.extend(&incremental_versions);
            all_versions.into_iter().collect()
        } else {
            complete_versions.into_iter().collect()
        };
        
        result.sort();
        Ok(result)
    }

    async fn get_patch_url(&self, channel: &str, os: &str, arch: &str, from_version: i32, to_version: i32) -> Result<String> {
        self.get_version_url(channel, os, arch, from_version, to_version).await
    }

    async fn has_complete_version(&self, channel: &str, os: &str, arch: &str, version: i32) -> Result<bool> {
        let data = self.fetch_data().await?;
        let channel_data = self.get_channel(&data, channel)?;
        let os_data = Self::get_files_for_current_os(channel_data)?;
        
        let expected_filename = format!("v{}-{}-{}.pwr", version, os, arch);
        Ok(os_data.contains_key(&expected_filename))
    }
}

impl Default for ShipOfYarnProvider {
    fn default() -> Self {
        Self::new()
    }
}
