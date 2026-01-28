use std::path::PathBuf;
use anyhow::Result;

/// Resolves the path to Assets.zip for a specific client version
/// This allows the dedicated server to use assets directly from the client installation
/// without copying the large Assets.zip file
pub fn resolve_client_assets_path(
    app_dir: &PathBuf,
    branch: &str,
    version: &str,
) -> Result<PathBuf> {
    let version_dir_name = if version == "latest" || version == "0" {
        "latest"
    } else {
        version
    };
    
    let client_version_dir = app_dir.join(branch).join(version_dir_name);
    let assets_path = client_version_dir.join("Assets.zip");
    
    if !assets_path.exists() {
        return Err(anyhow::anyhow!(
            "Assets.zip not found for client version {} in branch {}: {:?}",
            version,
            branch,
            assets_path
        ));
    }
    
    Ok(assets_path)
}

/// Finds the best matching client version for the server
/// Priority: exact version -> latest with matching version number -> error
pub fn find_best_client_version(
    app_dir: &PathBuf,
    branch: &str,
    target_version: &str,
) -> Result<String> {
    // If target is "latest" or "0", return "latest"
    if target_version == "latest" || target_version == "0" {
        return Ok("latest".to_string());
    }
    
    // Check for exact version match first
    let exact_version_dir = app_dir.join(branch).join(target_version);
    if exact_version_dir.exists() && exact_version_dir.join("Assets.zip").exists() {
        return Ok(target_version.to_string());
    }
    
    // Check if latest folder has the same version number
    let latest_dir = app_dir.join(branch).join("latest");
    if latest_dir.exists() {
        if let Ok(version_json_path) = get_version_json_path(app_dir, branch) {
            if let Ok(content) = std::fs::read_to_string(&version_json_path) {
                if let Ok(version_info) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(latest_version) = version_info.get("buildVersion").and_then(|v| v.as_str()) {
                        if latest_version == target_version {
                            return Ok("latest".to_string());
                        }
                    }
                }
            }
        }
    }
    
    Err(anyhow::anyhow!(
        "No matching client version found for {} in branch {}",
        target_version,
        branch
    ))
}

/// Gets the path to version.json for a branch
fn get_version_json_path(app_dir: &PathBuf, branch: &str) -> Result<PathBuf> {
    let version_json_path = app_dir.join(branch).join("version.json");
    if version_json_path.exists() {
        Ok(version_json_path)
    } else {
        Err(anyhow::anyhow!("version.json not found for branch {}", branch))
    }
}

/// Validates that a client version has all required files for server usage
pub fn validate_client_version(
    app_dir: &PathBuf,
    branch: &str,
    version: &str,
) -> Result<()> {
    let version_dir_name = if version == "latest" || version == "0" {
        "latest"
    } else {
        version
    };
    
    let client_dir = app_dir.join(branch).join(version_dir_name);
    
    // Check Assets.zip exists
    let assets_path = client_dir.join("Assets.zip");
    if !assets_path.exists() {
        return Err(anyhow::anyhow!("Assets.zip not found in {:?}", client_dir));
    }
    
    // Check Server folder exists (for HytaleServer.jar)
    let server_dir = client_dir.join("Server");
    if !server_dir.exists() {
        return Err(anyhow::anyhow!("Server folder not found in {:?}", client_dir));
    }
    
    let server_jar = server_dir.join("HytaleServer.jar");
    if !server_jar.exists() {
        return Err(anyhow::anyhow!("HytaleServer.jar not found in {:?}", server_dir));
    }
    
    Ok(())
}

/// Generates the server arguments with direct asset path
pub fn generate_server_args_with_direct_assets(
    base_args: &str,
    assets_path: &PathBuf,
) -> String {
    let mut args = base_args.to_string();
    
    // Remove existing --assets argument if present
    if let Some(pos) = args.find("--assets") {
        let start = args[..pos].rfind(' ').unwrap_or(0);
        let end = args[pos..].find(' ').map(|i| pos + i).unwrap_or(args.len());
        args.replace_range(start..end, "");
    }
    
    // Remove any standalone Assets.zip argument
    let words: Vec<&str> = args.split_whitespace().collect();
    let filtered_words: Vec<&str> = words.iter()
        .filter(|&word| !word.eq_ignore_ascii_case("Assets.zip"))
        .cloned()
        .collect();
    args = filtered_words.join(" ");
    
    // Convert to absolute path and add the direct assets argument
    let absolute_assets_path = assets_path.canonicalize().unwrap_or_else(|_| assets_path.clone());
    
    // Remove the \\?\ prefix that Windows adds for very long paths
    let path_str = absolute_assets_path.to_string_lossy();
    let clean_path = path_str.strip_prefix("\\\\?\\").unwrap_or(&path_str);
    
    if !args.is_empty() && !args.ends_with(' ') {
        args.push(' ');
    }
    args.push_str(&format!("--assets {}", clean_path));
    
    args.trim().to_string()
}
