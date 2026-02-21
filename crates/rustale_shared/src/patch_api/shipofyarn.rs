use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::patch_api::traits::PatchProvider;

const SHIPOFYARN_API_URL: &str = "https://thecute.cloud/ShipOfYarn/api.php";

#[derive(Debug, Serialize, Deserialize)]
struct ShipOfYarnLinkStore {
    #[serde(default)]
    patch: std::collections::HashMap<String, String>,
    #[serde(default)]
    base: std::collections::HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ShipOfYarnPlatform {
    linux: ShipOfYarnLinkStore,
    windows: ShipOfYarnLinkStore,
    mac: ShipOfYarnLinkStore,
}

#[derive(Debug, Serialize, Deserialize)]
struct ShipOfYarnHytale {
    release: ShipOfYarnPlatform,
    #[serde(rename = "pre-release")]
    pre_release: ShipOfYarnPlatform,
}

#[derive(Debug, Serialize, Deserialize)]
struct ShipOfYarnAPI {
    hytale: ShipOfYarnHytale,
}

/// ShipOfYarn API provider
pub struct ShipOfYarnProvider {
}

impl ShipOfYarnProvider {
    pub fn new() -> Self {
        Self {
        }
    }

    async fn fetch_data(&self) -> Result<ShipOfYarnAPI> {
        let response = crate::HTTP_CLIENT
            .get(SHIPOFYARN_API_URL)
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!("ShipOfYarn API error: {}", response.status());
        }

        let data: ShipOfYarnAPI = response.json().await?;
        Ok(data)
    }

    fn get_platform_data<'a>(&self, api: &'a ShipOfYarnAPI, channel: &str, os: &str) -> Option<&'a ShipOfYarnLinkStore> {
        let channel_section = if channel == "release" {
            &api.hytale.release
        } else {
            &api.hytale.pre_release
        };

        match os {
            "linux" => Some(&channel_section.linux),
            "windows" => Some(&channel_section.windows),
            "mac" | "darwin" => Some(&channel_section.mac),
            _ => None,
        }
    }

    fn parse_version_from_key(&self, key: &str) -> Option<i32> {
        // v19~20-windows-amd64.pwr -> 20
        // v1-windows-amd64.pwr -> 1
        if let Some(start) = key.find('v') {
            let part = &key[start+1..];
            if let Some(tilde) = part.find('~') {
                let to_part = &part[tilde+1..];
                if let Some(dash) = to_part.find('-') {
                    return to_part[..dash].parse().ok();
                }
            } else if let Some(dash) = part.find('-') {
                return part[..dash].parse().ok();
            }
        }
        None
    }
}

#[async_trait]
impl PatchProvider for ShipOfYarnProvider {
    fn name(&self) -> &str {
        "ShipOfYarn"
    }

    fn priority(&self) -> i32 {
        60 // High priority for official-like API
    }

    async fn is_available(&self) -> bool {
        match crate::HTTP_CLIENT.head(SHIPOFYARN_API_URL).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    async fn get_latest_version(&self, channel: &str, os: &str, _arch: &str) -> Result<i32> {
        let data = self.fetch_data().await?;
        let platform = self.get_platform_data(&data, channel, os)
            .ok_or_else(|| anyhow::anyhow!("Platform not found in API: {}", os))?;

        let mut latest = 0;
        
        // Check base versions (full)
        for key in platform.base.keys() {
            if let Some(v) = self.parse_version_from_key(key) {
                if v > latest { latest = v; }
            }
        }
        
        // Check patch versions
        for key in platform.patch.keys() {
            if let Some(v) = self.parse_version_from_key(key) {
                if v > latest { latest = v; }
            }
        }

        if latest == 0 {
            anyhow::bail!("No versions found for channel: {} and platform: {}", channel, os);
        }
        Ok(latest)
    }

    async fn get_available_versions(&self, channel: &str, os: &str, _arch: &str) -> Result<Vec<i32>> {
        let data = self.fetch_data().await?;
        let platform = self.get_platform_data(&data, channel, os)
            .ok_or_else(|| anyhow::anyhow!("Platform not found in API"))?;

        let mut versions = std::collections::HashSet::new();
        
        for key in platform.base.keys() {
            if let Some(v) = self.parse_version_from_key(key) {
                versions.insert(v);
            }
        }
        
        for key in platform.patch.keys() {
            if let Some(v) = self.parse_version_from_key(key) {
                versions.insert(v);
            }
        }

        let mut result: Vec<i32> = versions.into_iter().collect();
        result.sort();
        Ok(result)
    }

    async fn get_patch_url(
        &self,
        channel: &str,
        os: &str,
        _arch: &str,
        from_version: i32,
        to_version: i32,
    ) -> Result<String> {
        let data = self.fetch_data().await?;
        let platform = self.get_platform_data(&data, channel, os)
            .ok_or_else(|| anyhow::anyhow!("Platform not found in API"))?;

        let search_prefix = format!("v{}~{}", from_version, to_version);
        for (key, url) in &platform.patch {
            if key.starts_with(&search_prefix) {
                return Ok(url.clone());
            }
        }

        anyhow::bail!("Patch not found on ShipOfYarn: {}->{}", from_version, to_version)
    }

    async fn has_complete_version(
        &self,
        channel: &str,
        os: &str,
        _arch: &str,
        version_num: i32,
    ) -> Result<bool> {
        let data = self.fetch_data().await?;
        let platform = self.get_platform_data(&data, channel, os)
            .ok_or_else(|| anyhow::anyhow!("Platform not found in API"))?;

        let search_prefix = format!("v{}-", version_num);
        for key in platform.base.keys() {
            if key.starts_with(&search_prefix) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn get_complete_url(
        &self,
        channel: &str,
        os: &str,
        _arch: &str,
        version_num: i32,
    ) -> Result<String> {
        let data = self.fetch_data().await?;
        let platform = self.get_platform_data(&data, channel, os)
            .ok_or_else(|| anyhow::anyhow!("Platform not found in API"))?;

        let search_prefix = format!("v{}-", version_num);
        for (key, url) in &platform.base {
            if key.starts_with(&search_prefix) {
                return Ok(url.clone());
            }
        }

        anyhow::bail!("Complete version not found on ShipOfYarn for v{}", version_num)
    }
}
