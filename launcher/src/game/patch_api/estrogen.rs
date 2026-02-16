use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use anyhow::{Context, Result};
use std::collections::HashMap;

use super::traits::PatchProvider;
use super::utils::*;

const ESTROGEN_BASE_URL: &str = "https://licdn.estrogen.cat/hytale";

#[derive(Debug, Deserialize)]
pub struct EstrogenVersionInfo {
    pub version: i32,
    pub files: HashMap<String, EstrogenFileInfo>,
}

#[derive(Debug, Deserialize)]
pub struct EstrogenFileInfo {
    pub url: String,
    pub size: Option<u64>,
    pub checksum: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct EstrogenManifest {
    pub versions: HashMap<i32, EstrogenVersionInfo>,
    pub latest: HashMap<String, i32>, // channel -> latest version
}

/// Estrogen API provider (mirror API)
pub struct EstrogenProvider {
    client: Client,
}

impl EstrogenProvider {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }

    fn get_arch_name() -> &'static str {
        get_arch_name()
    }

    fn guess_patch_url_no_auth(&self, architecture: &str, operating_system: &str, channel: &str, start_version: i32, target_version: i32) -> String {
        format!("{}/patches/{}/{}/{}/{}/{}.pwr", 
            ESTROGEN_BASE_URL, operating_system, architecture, channel, start_version, target_version)
    }

    fn guess_patch_sig_url_no_auth(&self, architecture: &str, operating_system: &str, channel: &str, start_version: i32, target_version: i32) -> String {
        format!("{}/patches/{}/{}/{}/{}/{}.pwr.sig", 
            ESTROGEN_BASE_URL, operating_system, architecture, channel, start_version, target_version)
    }

    async fn check_version_exists(&self, start_version: i32, end_version: i32, architecture: &str, operating_system: &str, channel: &str) -> bool {
        let url = self.guess_patch_url_no_auth(architecture, operating_system, channel, start_version, end_version);
        check_file_exists(&self.client, &url).await
    }

    async fn find_latest_version(&self, current: i32, architecture: &str, operating_system: &str, channel: &str) -> i32 {
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

    async fn get_available_versions_search(&self, architecture: &str, operating_system: &str, channel: &str) -> Result<Vec<i32>> {
        let latest = self.find_latest_version(0, architecture, operating_system, channel).await;
        let mut versions = Vec::new();
        
        // Check versions from 0 to latest
        for v in 0..=latest {
            if self.check_version_exists(0, v, architecture, operating_system, channel).await {
                versions.push(v);
            }
        }

        if versions.is_empty() {
            anyhow::bail!("No versions found for channel {} on {}-{}", channel, operating_system, architecture);
        }

        Ok(versions)
    }
}

#[async_trait]
impl PatchProvider for EstrogenProvider {
    fn name(&self) -> &'static str {
        "estrogen"
    }

    async fn is_available(&self) -> bool {
        // Check if the base URL is accessible
        match self.client.head(format!("{}/", ESTROGEN_BASE_URL)).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    async fn get_latest_version(&self, channel: &str, os: &str, arch: &str) -> Result<i32> {
        let version = self.find_latest_version(0, arch, os, channel).await;
        Ok(version)
    }

    async fn get_available_versions(&self, channel: &str, os: &str, arch: &str) -> Result<Vec<i32>> {
        self.get_available_versions_search(arch, os, channel).await
    }

    async fn get_patch_url(&self, channel: &str, os: &str, arch: &str, from_version: i32, to_version: i32) -> Result<String> {
        let url = self.guess_patch_url_no_auth(arch, os, channel, from_version, to_version);
        
        // Verify the URL exists
        if !self.check_version_exists(from_version, to_version, arch, os, channel).await {
            anyhow::bail!("Patch {}->{} not found for channel {} on {}-{}", from_version, to_version, channel, os, arch);
        }
        
        Ok(url)
    }

    async fn get_patch_signature_url(&self, channel: &str, os: &str, arch: &str, from_version: i32, to_version: i32) -> Result<String> {
        let url = self.guess_patch_sig_url_no_auth(arch, os, channel, from_version, to_version);
        Ok(url)
    }

    async fn has_complete_version(&self, channel: &str, os: &str, arch: &str, version: i32) -> Result<bool> {
        let exists = self.check_version_exists(0, version, arch, os, channel).await;
        Ok(exists)
    }

    async fn get_jre_url(&self, os: &str, arch: &str) -> Result<String> {
        // Estrogen API provides JRE URLs from redist/jre/
        // We'll go directly to the platform directory and get the latest file
        
        let platform_url = format!("{}/redist/jre/{}/{}/", ESTROGEN_BASE_URL, os, arch);
        
        // Try to get directory listing for the platform-specific directory
        match self.client.get(&platform_url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let text = resp.text().await.unwrap_or_default();
                
                // Extract all filenames from directory listing
                let mut filenames = Vec::new();
                for line in text.lines() {
                    if let Some(filename) = extract_filename_from_html(line) {
                        // Only include actual JRE files
                        if looks_like_jre_file(&filename) {
                            filenames.push(filename);
                        }
                    }
                }
                
                if filenames.is_empty() {
                    anyhow::bail!("No JRE files found in platform directory: {}/{}", os, arch);
                }
                
                // Get the latest filename
                let latest_file = get_latest_filename(&filenames)
                    .ok_or_else(|| anyhow::anyhow!("No valid JRE files found"))?;
                
                Ok(format!("{}{}", platform_url, latest_file))
            }
            _ => {
                anyhow::bail!("Failed to access platform directory: {}", platform_url)
            }
        }
    }
    
    async fn get_butler_url(&self, os: &str, arch: &str) -> Result<String> {
        // Estrogen API doesn't provide Butler URLs
        anyhow::bail!("Butler URLs not available from Estrogen API")
    }
}

impl Default for EstrogenProvider {
    fn default() -> Self {
        Self::new()
    }
}
