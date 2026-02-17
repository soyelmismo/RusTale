use async_trait::async_trait;
use reqwest::Client;
use anyhow::Result;

use super::traits::PatchProvider;
use super::utils::*;

const ESTROGEN_BASE_URL: &str = "https://licdn.estrogen.cat/hytale";


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

    
    fn guess_patch_url_no_auth(&self, architecture: &str, operating_system: &str, channel: &str, start_version: i32, target_version: i32) -> String {
        format!("{}/patches/{}/{}/{}/{}/{}.pwr", 
            ESTROGEN_BASE_URL, operating_system, architecture, channel, start_version, target_version)
    }

    async fn check_version_exists(&self, start_version: i32, end_version: i32, architecture: &str, operating_system: &str, channel: &str) -> bool {
        let url = self.guess_patch_url_no_auth(architecture, operating_system, channel, start_version, end_version);
        check_file_exists(&self.client, &url).await
    }

    async fn get_available_versions_search(&self, architecture: &str, operating_system: &str, channel: &str) -> Result<Vec<i32>> {
        let latest = find_latest_version_generic(0, |version| async move {
            self.check_version_exists(0, version, architecture, operating_system, channel).await
        }).await;
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
        let version = find_latest_version_generic(0, |version| async move {
            self.check_version_exists(0, version, arch, os, channel).await
        }).await;
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

    async fn has_complete_version(&self, channel: &str, os: &str, arch: &str, version: i32) -> Result<bool> {
        let exists = self.check_version_exists(0, version, arch, os, channel).await;
        Ok(exists)
    }
}

impl Default for EstrogenProvider {
    fn default() -> Self {
        Self::new()
    }
}
