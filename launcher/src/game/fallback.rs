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
    println!("[Fallback] Raw response: {}", response_text);
    
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

/// Get latest version number for a channel
pub fn get_latest_version(fallback_data: &FallbackAPI, channel: &str) -> Result<i32> {
    let platform_data = match channel {
        "release" => &fallback_data.hytale.release,
        "pre-release" => &fallback_data.hytale.pre_release,
        _ => anyhow::bail!("Unsupported channel: {}", channel),
    };

    let os_name = std::env::consts::OS;
    let os_data = match os_name {
        "linux" => &platform_data.linux.files,
        "windows" => &platform_data.windows.files,
        "macos" => &platform_data.mac.files,
        _ => anyhow::bail!("Unsupported OS: {}", os_name),
    };

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
        anyhow::bail!("No versions found for channel {} on {}", channel, os_name);
    }

    Ok(max_version)
}

/// Get download URL for a specific version
pub fn get_version_url(fallback_data: &FallbackAPI, channel: &str, version: i32) -> Result<String> {
    let platform_data = match channel {
        "release" => &fallback_data.hytale.release,
        "pre-release" => &fallback_data.hytale.pre_release,
        _ => anyhow::bail!("Unsupported channel: {}", channel),
    };

    let os_name = std::env::consts::OS;
    let arch_name = get_arch_name();
    
    let os_data = match os_name {
        "linux" => &platform_data.linux.files,
        "windows" => &platform_data.windows.files,
        "macos" => &platform_data.mac.files,
        _ => anyhow::bail!("Unsupported OS: {}", os_name),
    };

    // Try different filename patterns
    let possible_filenames = match channel {
        "release" => vec![format!("v{}-{}-{}.pwr", version, os_name, arch_name)],
        "pre-release" => {
            // For pre-release, try patterns like "v19~20-linux-amd64.pwr"
            vec![
                format!("v{}~{}-{}-{}.pwr", version - 1, version, os_name, arch_name),
                format!("v{}~{}-{}-{}.pwr", version, version + 1, os_name, arch_name),
            ]
        },
        _ => vec![],
    };

    for filename in possible_filenames {
        if let Some(url) = os_data.get(&filename) {
            return Ok(url.clone());
        }
    }

    anyhow::bail!("Version {} not found for channel {} on {}-{}", version, channel, os_name, arch_name)
}

fn extract_version_from_filename(filename: &str) -> Option<i32> {
    // Extract version from patterns like:
    // - "v8-linux-amd64.pwr" -> 8
    // - "v19~20-linux-amd64.pwr" -> 20 (the higher version)
    
    if let Some(start) = filename.find('v') {
        let version_part = &filename[start + 1..];
        let version_part = version_part.split('-').next()?;
        
        if let Some(tilde_pos) = version_part.find('~') {
            // Pre-release format: "19~20" -> take the higher number (20)
            let second_part = &version_part[tilde_pos + 1..];
            second_part.parse().ok()
        } else {
            // Release format: "8" -> parse directly
            version_part.parse().ok()
        }
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
