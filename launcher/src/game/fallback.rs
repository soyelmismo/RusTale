use serde::Deserialize;
use anyhow::{Context, Result};
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct FallbackAPI {
    pub hytale: HytaleData,
    pub jre: JreData,
    pub butler: ButlerData,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct HytaleData {
    pub release: PlatformData,
    #[serde(rename = "pre-release")]
    pub pre_release: PlatformData,
}

#[derive(Debug, Deserialize, Default)]
pub struct PlatformData {
    pub linux: PlatformFiles,
    pub windows: PlatformFiles,
    pub mac: PlatformFiles,
}

#[derive(Debug, Deserialize, Default)]
pub struct PlatformFiles {
    #[serde(flatten)]
    pub files: HashMap<String, String>,
    #[allow(dead_code)]
    pub patch: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
pub struct JreData {
    pub linux: HashMap<String, String>,
    pub windows: HashMap<String, String>,
    pub mac: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct ButlerData {
    pub linux: HashMap<String, String>,
    pub windows: HashMap<String, String>,
    pub mac: HashMap<String, String>,
}

/// Fetch fallback data from alternative API
pub async fn fetch_fallback_data(client: &reqwest::Client) -> Result<FallbackAPI> {
    let response = client
        .get("https://thecute.cloud/ShipOfYarn/api.php")
        .send()
        .await?;

    if !response.status().is_success() {
        anyhow::bail!("Fallback API returned status: {}", response.status());
    }

    let response_text = response.text().await.context("Failed to read response body")?;
    // println!("[Fallback] Raw response: {}", response_text);
    
    let data: FallbackAPI = serde_json::from_str(&response_text)
        .map_err(|e| {
            println!("[Fallback] JSON parse error: {}", e);
            anyhow::anyhow!("Failed to decode fallback response: {}", e)
        })?;
    
    Ok(data)
}

/// Get Butler download URL for current platform
pub fn get_butler_url(fallback_data: &FallbackAPI) -> Result<String> {
    let os_name = std::env::consts::OS;
    let arch_name = get_arch_name();
    
    let platform_data = match os_name {
        "linux" => &fallback_data.butler.linux,
        "windows" => &fallback_data.butler.windows,
        "macos" => &fallback_data.butler.mac,
        _ => anyhow::bail!("Unsupported OS: {}", os_name),
    };

    let filename = match os_name {
        "linux" => "butler-linux-amd64.zip",
        "windows" => "butler-windows-amd64.zip",
        "macos" => "butler-mac-amd64.zip",
        _ => anyhow::bail!("Unsupported OS: {}", os_name),
    };

    platform_data
        .get(filename)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Butler not found for {}-{}", os_name, arch_name))
}

/// Get JRE download URL for current platform
pub fn get_jre_url(fallback_data: &FallbackAPI) -> Result<String> {
    let os_name = std::env::consts::OS;
    let arch_name = get_arch_name();
    
    let platform_data = match os_name {
        "linux" => &fallback_data.jre.linux,
        "windows" => &fallback_data.jre.windows,
        "macos" => &fallback_data.jre.mac,
        _ => anyhow::bail!("Unsupported OS: {}", os_name),
    };

    // Get first available JRE file for the platform
    let filename = platform_data
        .keys()
        .find(|key| {
            key.starts_with("OpenJDK") && 
            (key.contains(&format!("{}_{}", os_name, arch_name)) || 
             (os_name == "linux" && key.contains("x64_linux")) ||
             (os_name == "windows" && key.contains("x64_windows")) ||
             (os_name == "macos" && key.contains("aarch64_mac")))
        })
        .ok_or_else(|| anyhow::anyhow!("JRE not found for {}-{}", os_name, arch_name))?;

    platform_data
        .get(filename)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("JRE not found for {}-{}", os_name, arch_name))
}

/// Helper function to get platform data for a channel
fn get_platform_data<'a>(fallback_data: &'a FallbackAPI, channel: &str) -> Result<&'a PlatformData> {
    match channel {
        "release" => Ok(&fallback_data.hytale.release),
        "pre-release" => Ok(&fallback_data.hytale.pre_release),
        _ => anyhow::bail!("Unsupported channel: {}", channel),
    }
}

/// Helper function to get OS files for a platform
fn get_os_files(platform_data: &PlatformData) -> Result<&std::collections::HashMap<String, String>> {
    let os_name = std::env::consts::OS;
    match os_name {
        "linux" => Ok(&platform_data.linux.files),
        "windows" => Ok(&platform_data.windows.files),
        "macos" => Ok(&platform_data.mac.files),
        _ => anyhow::bail!("Unsupported OS: {}", os_name),
    }
}

/// Get latest version number for a channel
pub fn get_latest_version(fallback_data: &FallbackAPI, channel: &str) -> Result<i32> {
    let platform_data = get_platform_data(fallback_data, channel)?;
    let os_data = get_os_files(platform_data)?;

    // Extract version numbers from filenames like "v8-linux-amd64.pwr" or "v19~20-linux-amd64.pwr"
    let mut max_version = 0;
    for filename in os_data.keys() {
        if let Some(version_str) = extract_version_from_filename(filename) {
            if version_str > max_version {
                max_version = version_str;
            }
        }
    }

    if max_version == 0 {
        let os_name = std::env::consts::OS;
        anyhow::bail!("No versions found for channel {} on {}", channel, os_name);
    }

    Ok(max_version)
}

/// Get all available versions for a channel from fallback API
pub fn get_all_available_versions(fallback_data: &FallbackAPI, channel: &str) -> Result<Vec<i32>> {
    let platform_data = get_platform_data(fallback_data, channel)?;
    let os_data = get_os_files(platform_data)?;

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
        let os_name = std::env::consts::OS;
        anyhow::bail!("No versions found for channel {} on {}", channel, os_name);
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
    println!("Available versions for {}: {:?}", channel, result);
    Ok(result)
}

/// Check if a complete version exists for the target
pub fn has_complete_version(fallback_data: &FallbackAPI, channel: &str, target_version: i32) -> bool {
    let platform_data = get_platform_data(fallback_data, channel);
    let os_data = match platform_data {
        Ok(data) => get_os_files(data),
        Err(_) => return false,
    };
    
    let os_name = std::env::consts::OS;
    let arch_name = get_arch_name();
    let expected_filename = format!("v{}-{}-{}.pwr", target_version, os_name, arch_name);
    
    match os_data {
        Ok(data) => {
            let has_complete = data.contains_key(&expected_filename);
            println!("DEBUG: Looking for complete version file: '{}', found: {}", expected_filename, has_complete);
            has_complete
        }
        Err(_) => false
    }
}

/// Get download URL for a specific version patch
pub fn get_version_url(fallback_data: &FallbackAPI, channel: &str, prev_version: i32, target_version: i32) -> Result<String> {
    let platform_data = get_platform_data(fallback_data, channel)?;
    let os_data = get_os_files(platform_data)?;

    let os_name = std::env::consts::OS;
    let arch_name = get_arch_name();

    // Try different filename patterns
    let possible_filenames = vec![
        // Complete version: "v8-linux-amd64.pwr"
        format!("v{}-{}-{}.pwr", target_version, os_name, arch_name),
        // Incremental: "v7~8-linux-amd64.pwr"
        format!("v{}~{}-{}-{}.pwr", prev_version, target_version, os_name, arch_name),
    ];

    for filename in possible_filenames {
        if let Some(url) = os_data.get(&filename) {
            println!("Found fallback URL for {}->{}: {}", prev_version, target_version, filename);
            return Ok(url.clone());
        }
    }

    anyhow::bail!("Patch {}->{} not found for channel {} on {}-{}", prev_version, target_version, channel, os_name, arch_name)
}

fn extract_versions_from_filename(filename: &str) -> Option<(i32, i32)> {
    // Extract version from patterns like:
    // - "v8-linux-amd64.pwr" -> (0, 8) (complete version)
    // - "v19~20-linux-amd64.pwr" -> (19, 20) (incremental)
    
    if let Some(start) = filename.find('v') {
        let version_part = &filename[start + 1..];
        let version_part = version_part.split('-').next()?;
        
        if let Some(tilde_pos) = version_part.find('~') {
            // Incremental format: "19~20" -> (19, 20)
            let from_part = &version_part[..tilde_pos];
            let to_part = &version_part[tilde_pos + 1..];
            match (from_part.parse::<i32>(), to_part.parse::<i32>()) {
                (Ok(from), Ok(to)) => Some((from, to)),
                _ => None,
            }
        } else {
            // Complete format: "8" -> (0, 8)
            match version_part.parse::<i32>() {
                Ok(version) => Some((0, version)),
                _ => None,
            }
        }
    } else {
        None
    }
}

fn extract_version_from_filename(filename: &str) -> Option<i32> {
    // Extract version from patterns like:
    // - "v8-linux-amd64.pwr" -> 8
    // - "v19~20-linux-amd64.pwr" -> 20 (the higher version)
    
    if let Some((_, to)) = extract_versions_from_filename(filename) {
        Some(to)
    } else {
        None
    }
}

fn get_arch_name() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    }
}
