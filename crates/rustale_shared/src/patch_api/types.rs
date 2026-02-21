use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::fs;

/// Game version information structure
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GameVersionInfo {
    pub user_version: i32,
    pub current_local: i32,
    pub latest_remote: i32,
    pub available_versions: Vec<i32>,
    pub available_versions_from_fallback: Option<Vec<i32>>,
    pub update_available: bool,
}

pub async fn get_local_version(base_dir: &PathBuf, channel: &str) -> Result<i32> {
    let version_file = base_dir.join(channel).join("version.json");
    if !version_file.exists() { 
        return Ok(0); 
    }

    let content = fs::read_to_string(&version_file).await
        .context("Version file unreadable")?;
    
    let version_info: serde_json::Value = serde_json::from_str(&content)
        .unwrap_or_else(|_| serde_json::json!({ "version": 0 }));

    Ok(version_info.get("version").and_then(|v| v.as_i64()).unwrap_or(0) as i32)
}

pub async fn save_local_version(base_dir: &PathBuf, channel: &str, version: i32) -> Result<()> {
    let version_file = base_dir.join(channel).join("version.json");
    if let Some(parent) = version_file.parent() {
        fs::create_dir_all(parent).await?;
    }
    let version_info = serde_json::json!({ "version": version });
    fs::write(&version_file, serde_json::to_string_pretty(&version_info)?).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_get_local_version_no_file() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let base_dir = PathBuf::from(temp_dir.path());

        let version = get_local_version(&base_dir, "release").await.expect("Failed to get version");
        assert_eq!(version, 0); // Default when file doesn't exist
    }

    #[tokio::test]
    async fn test_save_and_get_local_version() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let base_dir = PathBuf::from(temp_dir.path());

        // Save version
        save_local_version(&base_dir, "release", 42).await.expect("Failed to save version");

        // Read it back
        let version = get_local_version(&base_dir, "release").await.expect("Failed to get version");
        assert_eq!(version, 42);
    }

    #[tokio::test]
    async fn test_save_local_version_creates_directory() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let base_dir = PathBuf::from(temp_dir.path());

        // Directory shouldn't exist yet
        assert!(!base_dir.join("beta").exists());

        // Save should create it
        save_local_version(&base_dir, "beta", 10).await.expect("Failed to save version");

        // Now it should exist
        assert!(base_dir.join("beta").exists());
        assert!(base_dir.join("beta/version.json").exists());
    }

    #[tokio::test]
    async fn test_get_local_version_invalid_json() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let base_dir = PathBuf::from(temp_dir.path());

        // Create directory and write invalid JSON
        let version_path = base_dir.join("release").join("version.json");
        fs::create_dir_all(version_path.parent().unwrap()).await.expect("Failed to create dir");
        fs::write(&version_path, "not valid json").await.expect("Failed to write file");

        // Should return 0 for invalid JSON
        let version = get_local_version(&base_dir, "release").await.expect("Failed to get version");
        assert_eq!(version, 0);
    }

    #[tokio::test]
    async fn test_version_file_format() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let base_dir = PathBuf::from(temp_dir.path());

        save_local_version(&base_dir, "test", 123).await.expect("Failed to save version");

        // Read the raw file content
        let content = fs::read_to_string(base_dir.join("test/version.json")).await.expect("Failed to read file");
        
        // Verify it's valid JSON with the expected structure
        let json: serde_json::Value = serde_json::from_str(&content).expect("Invalid JSON");
        assert_eq!(json.get("version").and_then(|v| v.as_i64()), Some(123));
    }
}
