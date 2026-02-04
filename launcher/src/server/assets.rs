use anyhow::Result;
use std::path::PathBuf;

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
                    // Get target version as number if possible for easier comparison
                    let target_ver_num = target_version.parse::<i64>().ok();

                    // Check "version" (Launcher's standard)
                    let matched =
                        if let Some(v_num) = version_info.get("version").and_then(|v| v.as_i64()) {
                            target_ver_num == Some(v_num)
                        } else {
                            false
                        };

                    if matched {
                        return Ok("latest".to_string());
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
        Err(anyhow::anyhow!(
            "version.json not found for branch {}",
            branch
        ))
    }
}

/// Validates that a client version has all required files for server usage
pub fn validate_client_version(app_dir: &PathBuf, branch: &str, version: &str) -> Result<()> {
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
        return Err(anyhow::anyhow!(
            "Server folder not found in {:?}",
            client_dir
        ));
    }

    let server_jar = server_dir.join("HytaleServer.jar");
    if !server_jar.exists() {
        return Err(anyhow::anyhow!(
            "HytaleServer.jar not found in {:?}",
            server_dir
        ));
    }

    Ok(())
}

/// Generates the server arguments with direct asset path
pub fn generate_server_args_with_direct_assets(base_args: &str, assets_path: &PathBuf) -> String {
    let mut args = base_args.to_string();

    // Remove existing --assets arguments and their values if present
    while let Some(pos) = args.find("--assets") {
        let start = args[..pos].rfind(' ').unwrap_or(0);

        // Find the end of the argument value (skip the flag and the path)
        let after_flag = pos + "--assets".len();
        // Skip whitespace after flag
        let val_start = args[after_flag..]
            .find(|c: char| !c.is_whitespace())
            .map(|i| after_flag + i)
            .unwrap_or(args.len());
        // Find end of value (next whitespace or end of string)
        let end = args[val_start..]
            .find(|c: char| c.is_whitespace())
            .map(|i| val_start + i)
            .unwrap_or(args.len());

        args.replace_range(start..end, "");
    }

    // Remove any standalone or orphaned Assets.zip path arguments
    let words: Vec<&str> = args.split_whitespace().collect();
    let filtered_words: Vec<&str> = words
        .into_iter()
        .filter(|word| {
            let normalized = word.replace('\\', "/");
            !normalized.ends_with("/Assets.zip") && !normalized.eq_ignore_ascii_case("Assets.zip")
        })
        .collect();
    args = filtered_words.join(" ");

    // Convert to absolute path and add the direct assets argument
    let absolute_assets_path = assets_path
        .canonicalize()
        .unwrap_or_else(|_| assets_path.clone());

    // Remove the \\?\ prefix that Windows adds for very long paths
    let path_str = absolute_assets_path.to_string_lossy();
    let clean_path = path_str.strip_prefix("\\\\?\\").unwrap_or(&path_str);

    if !args.is_empty() && !args.ends_with(' ') {
        args.push(' ');
    }
    args.push_str(&format!("--assets {}", clean_path));

    args.trim().to_string()
}
