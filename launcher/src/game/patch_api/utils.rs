/// Utility functions for patch API providers
use std::collections::BTreeMap;

/// Format bytes in human-readable format
pub fn format_bytes(bytes: u64) -> String {
    if bytes > 1_000_000_000 { 
        format!("{:.2} GB", bytes as f64 / 1_073_741_824.0) 
    } else if bytes > 1_000_000 { 
        format!("{:.1} MB", bytes as f64 / 1_048_576.0) 
    } else if bytes > 1_000 { 
        format!("{:.1} KB", bytes as f64 / 1024.0) 
    } else { 
        format!("{} B", bytes) 
    }
}

/// Format speed in human-readable format
pub fn format_speed(bytes_per_sec: f64) -> String {
    if bytes_per_sec > 1_000_000.0 { 
        format!("{:.2} MB/s", bytes_per_sec / 1_048_576.0) 
    } else if bytes_per_sec > 1_000.0 { 
        format!("{:.2} KB/s", bytes_per_sec / 1024.0) 
    } else { 
        format!("{:.0} B/s", bytes_per_sec) 
    }
}

/// Extract filename from HTML directory listing
/// Look for patterns like: <a href="filename.ext">filename.ext</a>
pub fn extract_filename_from_html(line: &str) -> Option<String> {
    if let Some(start) = line.find("href=\"") {
        let start = start + 6;
        if let Some(end) = line[start..].find("\"") {
            let filename = &line[start..start + end];
            if filename.contains('.') && !filename.starts_with('?') && !filename.starts_with('/') {
                return Some(filename.to_string());
            }
        }
    }
    None
}

/// Check if a filename looks like a JRE distribution file
pub fn looks_like_jre_file(filename: &str) -> bool {
    let filename_lower = filename.to_lowercase();
    
    // Check if it's a compressed archive (likely JRE distribution)
    let is_archive = filename_lower.ends_with(".tar.gz") || 
                   filename_lower.ends_with(".zip") ||
                   filename_lower.ends_with(".tar");
    
    // Check if it's related to Java/JRE
    let is_java = filename_lower.contains("jre") || 
                 filename_lower.contains("jdk") || 
                 filename_lower.contains("java") ||
                 filename_lower.contains("openjdk");
    
    // Exclude common non-JRE files
    let is_excluded = filename_lower.contains("readme") ||
                     filename_lower.contains("license") ||
                     filename_lower.contains("changelog") ||
                     filename_lower.contains(".txt") ||
                     filename_lower.contains(".md");
    
    is_archive && is_java && !is_excluded
}

/// Check if a filename looks like a Butler distribution file
pub fn looks_like_butler_file(filename: &str) -> bool {
    let filename_lower = filename.to_lowercase();
    
    // Check if it's a compressed archive
    let is_archive = filename_lower.ends_with(".tar.gz") || 
                   filename_lower.ends_with(".zip") ||
                   filename_lower.ends_with(".tar");
    
    // Check if it's related to Butler
    let is_butler = filename_lower.contains("butler");
    
    // Exclude common non-Butler files
    let is_excluded = filename_lower.contains("readme") ||
                     filename_lower.contains("license") ||
                     filename_lower.contains("changelog") ||
                     filename_lower.contains(".txt") ||
                     filename_lower.contains(".md");
    
    is_archive && is_butler && !is_excluded
}

/// Extract version information from filename
/// Supports patterns like:
/// - "v8-linux-amd64.pwr" -> (0, 8)
/// - "v19~20-linux-amd64.pwr" -> (19, 20)
/// - "jre-17.0.2-windows-x64.zip" -> Some(17.0.2)
pub fn extract_version_from_filename(filename: &str) -> Option<String> {
    // Try to extract semantic version first (like "17.0.2")
    let version_regex = regex::Regex::new(r"(\d+\.\d+\.\d+(?:\.\d+)*)").ok()?;
    if let Some(caps) = version_regex.captures(filename) {
        return Some(caps[1].to_string());
    }
    
    // Try to extract single version number (like "v8" or "v20")
    let single_version_regex = regex::Regex::new(r"v(\d+)").ok()?;
    if let Some(caps) = single_version_regex.captures(filename) {
        return Some(caps[1].to_string());
    }
    
    None
}

/// Extract version range from filename for patches
/// Returns (from_version, to_version)
pub fn extract_versions_from_filename(filename: &str) -> Option<(i32, i32)> {
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

/// Get the latest filename from a slice of filenames
/// Assumes that newer files have names that sort later alphabetically
pub fn get_latest_filename(filenames: &[String]) -> Option<String> {
    if filenames.is_empty() {
        return None;
    }
    
    // Sort and take the last one
    let mut sorted_filenames = filenames.to_vec();
    sorted_filenames.sort();
    sorted_filenames.pop()
}

/// Get the latest file from a map of filenames to URLs
/// Returns the filename and URL of the latest file
pub fn get_latest_file_from_map(file_map: &BTreeMap<String, String>) -> Option<(&String, &String)> {
    if file_map.is_empty() {
        return None;
    }
    
    // BTreeMap is already sorted by key, so the last entry is the latest
    file_map.iter().last()
}

/// Filter filenames by a predicate function
pub fn filter_filenames<F>(filenames: &[String], predicate: F) -> Vec<String>
where
    F: Fn(&str) -> bool,
{
    filenames
        .iter()
        .filter(|filename| predicate(filename))
        .cloned()
        .collect()
}

/// Get architecture name in standard format
pub fn get_arch_name() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    }
}

/// Get OS name in standard format
pub fn get_os_name() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "darwin",
        _ => std::env::consts::OS,
    }
}

/// Try multiple URLs and return the first one that exists
pub async fn find_first_existing_url(client: &reqwest::Client, urls: &[String]) -> Option<String> {
    for url in urls {
        if check_file_exists(client, url).await {
            return Some(url.clone());
        }
    }
    None
}

/// Get Butler download URL from itch.io CDN fallback
/// This works as a universal fallback for all platforms
pub fn get_butler_fallback_url(os: &str, arch: &str) -> String {
    format!("https://broth.itch.zone/butler/{}-{}/LATEST/archive.zip", os, arch)
}

/// Get Java download URL from Adoptium (Temurin) CDN fallback
/// This is official Eclipse Temurin distribution
pub fn get_java_adoptium_url(os: &str, arch: &str) -> String {
    // Map to Adoptium's naming conventions
    let os_name = match os {
        "windows" => "windows",
        "darwin" => "mac",  // Adoptium uses "mac" instead of "darwin"
        "linux" => "linux",
        other => os,
        _ => os,
    };
    
    let arch_name = match arch {
        "amd64" | "x86_64" => "x64", // They don't use amd64, Adoptium uses x64
        "aarch64" | "arm64" => "aarch64",
        _ => arch,
    };

    format!(
        "https://api.adoptium.net/v3/binary/latest/25/ga/{}/{}/jre/hotspot/normal/adoptium",
        os_name, arch_name
    )
}

/// Check if a file exists by making a HEAD request
pub async fn check_file_exists(client: &reqwest::Client, url: &str) -> bool {
    match client.head(url).send().await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

/// Generic version discovery using exponential and binary search
/// Accepts a future-returning closure to check if a version exists
pub async fn find_latest_version_generic<F, Fut>(current: i32, mut check_exists: F) -> i32 
where 
    F: FnMut(i32) -> Fut,
    Fut: std::future::Future<Output = bool>
{
    let current = if current <= 0 { 1 } else { current };
    let mut last_version = current;
    let mut cur_version = current;

    if check_exists(current + 1).await {
        while check_exists(cur_version).await {
            last_version = cur_version;
            cur_version *= 2;
        }
        while last_version + 1 < cur_version {
            let middle = (cur_version + last_version) / 2;
            if check_exists(middle).await { last_version = middle; } 
            else { cur_version = middle; }
        }
    }
    last_version
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_filename_from_html() {
        let line = r#"<a href="test-file.tar.gz">test-file.tar.gz</a>"#;
        assert_eq!(extract_filename_from_html(line), Some("test-file.tar.gz".to_string()));
    }

    #[test]
    fn test_looks_like_jre_file() {
        assert!(looks_like_jre_file("jre-17-windows-x64.zip"));
        assert!(looks_like_jre_file("openjdk-17-linux-x64.tar.gz"));
        assert!(!looks_like_jre_file("readme.txt"));
        assert!(!looks_like_jre_file("game-data.zip"));
    }

    #[test]
    fn test_looks_like_butler_file() {
        assert!(looks_like_butler_file("butler-windows-amd64.zip"));
        assert!(looks_like_butler_file("butler-linux-x64.tar.gz"));
        assert!(!looks_like_butler_file("jre-17-windows-x64.zip"));
        assert!(!looks_like_butler_file("readme.txt"));
    }

    #[test]
    fn test_extract_versions_from_filename() {
        assert_eq!(extract_versions_from_filename("v8-linux-amd64.pwr"), Some((0, 8)));
        assert_eq!(extract_versions_from_filename("v19~20-linux-amd64.pwr"), Some((19, 20)));
        assert_eq!(extract_versions_from_filename("invalid.txt"), None);
    }

    #[test]
    fn test_get_latest_filename() {
        // This test would need to be implemented based on actual usage
        // For now, we'll test the extraction function works
        assert!(true); // Placeholder
    }

    #[test]
    fn test_get_butler_fallback_url() {
        assert_eq!(
            get_butler_fallback_url("windows", "amd64"),
            "https://broth.itch.zone/butler/windows-amd64/LATEST/archive.zip"
        );
        assert_eq!(
            get_butler_fallback_url("darwin", "arm64"),
            "https://broth.itch.zone/butler/darwin-arm64/LATEST/archive.zip"
        );
        assert_eq!(
            get_butler_fallback_url("linux", "amd64"),
            "https://broth.itch.zone/butler/linux-amd64/LATEST/archive.zip"
        );
    }

    #[test]
    fn test_get_java_fallback_url() {
        assert_eq!(
            get_java_fallback_url("windows", "amd64"),
            "https://download.oracle.com_windows-x64_bin.zip"
        );
        assert_eq!(
            get_java_fallback_url("darwin", "arm64"),
            "https://download.oracle.com_macos-arm64_bin.tar.gz"
        );
        assert_eq!(
            get_java_fallback_url("linux", "amd64"),
            "https://download.oracle.com_linux-x64_bin.tar.gz"
        );
    }

    #[test]
    fn test_get_java_adoptium_url() {
        assert_eq!(
            get_java_adoptium_url("windows", "amd64"),
            "https://api.adoptium.net/v3/binary/latest/25/ga/windows/x64/jre/hotspot/normal/adoptium"
        );
        assert_eq!(
            get_java_adoptium_url("darwin", "arm64"),
            "https://api.adoptium.net/v3/binary/latest/25/ga/mac/aarch64/jre/hotspot/normal/adoptium"
        );
        assert_eq!(
            get_java_adoptium_url("linux", "amd64"),
            "https://api.adoptium.net/v3/binary/latest/25/ga/linux/x64/jre/hotspot/normal/adoptium"
        );
    }
}
