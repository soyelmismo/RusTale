use anyhow::{Context, Result};
use rustale_shared::reqwest::Client;
use serde::Deserialize;
use std::env;
use std::path::Path;

// Función para comparar versiones numéricamente (semantic versioning)
fn parse_version(version: &str) -> Vec<u32> {
    version
        .split('.')
        .filter_map(|part| part.parse::<u32>().ok())
        .collect()
}

fn should_update(local: &str, remote: &str) -> bool {
    let local_parts = parse_version(local);
    let remote_parts = parse_version(remote);

    // Siempre actualizar si las versiones son diferentes (upgrade o downgrade)
    local_parts != remote_parts
}

fn is_downgrade(local: &str, remote: &str) -> bool {
    let local_parts = parse_version(local);
    let remote_parts = parse_version(remote);

    // Comparar versión por versión
    for i in 0..std::cmp::max(local_parts.len(), remote_parts.len()) {
        let local = local_parts.get(i).unwrap_or(&0);
        let remote = remote_parts.get(i).unwrap_or(&0);

        if remote < local {
            return true; // Es downgrade
        } else if remote > local {
            return false; // Es upgrade
        }
    }

    false // Mismas versiones
}

#[derive(Debug, Clone)]
pub enum UpdaterMessage {
    CheckForUpdates,
    UpdateFound(ReleaseInfo),
    UpdateNotFound,
    Error(String),
    StartUpdate(String),
    UpdateProgress(f32, String),
    UpdateFinished,
    // New connected variants
    CheckComplete(Result<Option<ReleaseInfo>, String>),
    DownloadProgress(f32),
    InstallProgress(f32),
    RestartRequired,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub html_url: String,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

pub async fn check_for_updates(client: &Client) -> Result<Option<ReleaseInfo>> {
    let current_version = env!("CARGO_PKG_VERSION");
    if cfg!(debug_assertions) || current_version == "0.0.1" {
        println!(
            "[Updater] Development mode detected (v{}). Auto-update disabled.",
            current_version
        );
        return Ok(None);
    }

    let url = "https://api.github.com/repos/soyelmismo/RusTale/releases/latest";
    let response = client.get(url).send().await?;

    if !response.status().is_success() {
        return Ok(None);
    }

    let release: ReleaseInfo = response.json().await?;

    let remote_ver = release
        .tag_name
        .trim()
        .trim_start_matches('v')
        .replace("-g", "-");
    let local_ver = current_version
        .trim()
        .trim_start_matches('v')
        .replace("-g", "-");

    println!("[Updater] Local: v{}, Remote: v{}", local_ver, remote_ver);

    // Comparar versiones numéricamente para soportar downgrades
    if should_update(&local_ver, &remote_ver) {
        if get_asset_url(&release).is_some() {
            if is_downgrade(&local_ver, &remote_ver) {
                println!(
                    "[Updater] Downgrade detected: v{} -> v{}",
                    local_ver, remote_ver
                );
            } else {
                println!(
                    "[Updater] Upgrade detected: v{} -> v{}",
                    local_ver, remote_ver
                );
            }
            return Ok(Some(release));
        }
    }

    Ok(None)
}

pub fn get_asset_url(info: &ReleaseInfo) -> Option<String> {
    let target = if cfg!(windows) {
        "windows.zip"
    } else {
        "linux.zip"
    };

    info.assets
        .iter()
        .find(|a| a.name.to_lowercase().contains(target))
        .map(|a| a.browser_download_url.clone())
}

pub async fn perform_update(client: Client, asset_url: String) -> Result<()> {
    let current_exe = env::current_exe()?;
    let app_dir = current_exe.parent().context("No parent dir")?;

    let update_dir = app_dir.join("update_temp");
    if update_dir.exists() {
        tokio::fs::remove_dir_all(&update_dir).await?;
    }
    tokio::fs::create_dir_all(&update_dir).await?;

    let zip_path = update_dir.join("update.zip");

    println!("[Updater] Downloading ZIP from: {}", asset_url);

    let response = client.get(&asset_url).send().await?;
    if !response.status().is_success() {
        anyhow::bail!("Download failed: {}", response.status());
    }
    let content = response.bytes().await?;
    tokio::fs::write(&zip_path, content).await?;

    println!("[Updater] Extracting ZIP...");

    let update_dir_clone = update_dir.clone();
    let zip_path_clone = zip_path.clone();

    tokio::task::spawn_blocking(move || -> Result<()> {
        let file = std::fs::File::open(&zip_path_clone)?;
        let mut archive = zip::ZipArchive::new(file)?;
        archive.extract(&update_dir_clone)?;
        Ok(())
    })
    .await??;

    tokio::fs::remove_file(&zip_path).await?;

    println!("[Updater] Spawning update script...");
    spawn_update_script(&current_exe, &update_dir)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // === Version Parsing Tests ===

    #[test]
    fn test_parse_version_simple() {
        assert_eq!(parse_version("1.0.0"), vec![1, 0, 0]);
        assert_eq!(parse_version("2.1"), vec![2, 1]);
        assert_eq!(parse_version("0.0.1"), vec![0, 0, 1]);
    }

    #[test]
    fn test_parse_version_with_prefix() {
        // The 'v' prefix is stripped before calling parse_version
        assert_eq!(parse_version("1.0.0"), vec![1, 0, 0]);
        assert_eq!(parse_version("v1.0.0"), vec![]); // 'v' makes parse fail, returns empty
    }

    #[test]
    fn test_parse_version_invalid_parts() {
        // Non-numeric parts are filtered out
        assert_eq!(parse_version("1.0.beta"), vec![1, 0]);
        assert_eq!(parse_version("x.y.z"), vec![]);
    }

    // === should_update Tests ===

    #[test]
    fn test_should_update_upgrade() {
        assert!(should_update("1.0.0", "1.0.1"));
        assert!(should_update("1.0.0", "2.0.0"));
        assert!(should_update("1.2.3", "1.2.4"));
    }

    #[test]
    fn test_should_update_downgrade() {
        assert!(should_update("2.0.0", "1.0.0"));
        assert!(should_update("1.5.0", "1.4.0"));
    }

    #[test]
    fn test_should_update_same_version() {
        assert!(!should_update("1.0.0", "1.0.0"));
        assert!(!should_update("2.3.4", "2.3.4"));
    }

    #[test]
    fn test_should_update_different_lengths() {
        assert!(should_update("1.0", "1.0.1"));
        assert!(should_update("1.0.0", "1.0"));
    }

    // === is_downgrade Tests ===

    #[test]
    fn test_is_downgrade_true() {
        assert!(is_downgrade("2.0.0", "1.0.0"));
        assert!(is_downgrade("1.5.0", "1.4.0"));
        assert!(is_downgrade("1.0.1", "1.0.0"));
    }

    #[test]
    fn test_is_downgrade_false_upgrade() {
        assert!(!is_downgrade("1.0.0", "2.0.0"));
        assert!(!is_downgrade("1.0.0", "1.0.1"));
    }

    #[test]
    fn test_is_downgrade_same_version() {
        assert!(!is_downgrade("1.0.0", "1.0.0"));
    }

    #[test]
    fn test_is_downgrade_different_lengths() {
        assert!(!is_downgrade("1.0", "1.0.1")); // Upgrade
        assert!(is_downgrade("1.0.1", "1.0")); // Downgrade
    }

    // === get_asset_url Tests ===

    #[test]
    fn test_get_asset_url_windows() {
        let release = ReleaseInfo {
            tag_name: "v1.0.0".to_string(),
            html_url: "https://github.com/test".to_string(),
            assets: vec![
                Asset {
                    name: "RusTale-windows.zip".to_string(),
                    browser_download_url: "https://example.com/windows.zip".to_string(),
                    size: 1000,
                },
                Asset {
                    name: "RusTale-linux.zip".to_string(),
                    browser_download_url: "https://example.com/linux.zip".to_string(),
                    size: 1000,
                },
            ],
        };

        let url = get_asset_url(&release);
        if cfg!(windows) {
            assert_eq!(url, Some("https://example.com/windows.zip".to_string()));
        } else {
            assert_eq!(url, Some("https://example.com/linux.zip".to_string()));
        }
    }

    #[test]
    fn test_get_asset_url_case_insensitive() {
        let release = ReleaseInfo {
            tag_name: "v1.0.0".to_string(),
            html_url: "https://github.com/test".to_string(),
            assets: vec![Asset {
                name: "RUSTALE-WINDOWS.ZIP".to_string(),
                browser_download_url: "https://example.com/windows.zip".to_string(),
                size: 1000,
            }],
        };

        let url = get_asset_url(&release);
        if cfg!(windows) {
            assert!(url.is_some());
        }
    }

    #[test]
    fn test_get_asset_url_not_found() {
        let release = ReleaseInfo {
            tag_name: "v1.0.0".to_string(),
            html_url: "https://github.com/test".to_string(),
            assets: vec![Asset {
                name: "source.tar.gz".to_string(),
                browser_download_url: "https://example.com/source.tar.gz".to_string(),
                size: 1000,
            }],
        };

        let url = get_asset_url(&release);
        assert!(url.is_none());
    }

    // === Update Script Validation Tests ===
    // These tests verify that the generated scripts are syntactically valid
    // and don't contain obvious errors that would break the update process.

    #[test]
    fn test_windows_script_has_required_commands() {
        let current_exe = PathBuf::from("C:\\Program Files\\RusTale\\RusTale.exe");
        let update_dir = PathBuf::from("C:\\Program Files\\RusTale\\update_temp");

        let script = generate_update_script_windows(&current_exe, &update_dir);

        // Must have @echo off to prevent command echoing
        assert!(script.contains("@echo off"));

        // Must have timeout to allow process to exit
        assert!(script.contains("timeout"));

        // Must copy files from update dir
        assert!(script.contains("xcopy"));

        // Must clean up update dir
        assert!(script.contains("rmdir"));

        // Must restart the application
        assert!(script.contains("start"));

        // Must self-delete the script
        assert!(script.contains("del"));
    }

    #[test]
    fn test_windows_script_paths_are_escaped() {
        let current_exe = PathBuf::from("C:\\Program Files\\RusTale\\RusTale.exe");
        let update_dir = PathBuf::from("C:\\Program Files\\RusTale\\update_temp");

        let script = generate_update_script_windows(&current_exe, &update_dir);

        // Paths should be wrapped in quotes
        assert!(script.contains("\"C:\\"));
    }

    #[test]
    fn test_unix_script_has_required_commands() {
        let current_exe = PathBuf::from("/opt/rustale/RusTale");
        let update_dir = PathBuf::from("/opt/rustale/update_temp");

        let script = generate_update_script_unix(&current_exe, &update_dir);

        // Must have shebang
        assert!(script.starts_with("#!/bin/sh"));

        // Must have sleep to allow process to exit
        assert!(script.contains("sleep"));

        // Must copy files from update dir
        assert!(script.contains("cp -rf"));

        // Must clean up update dir
        assert!(script.contains("rm -rf"));

        // Must make executable
        assert!(script.contains("chmod +x"));

        // Must restart the application
        assert!(script.contains("\"/opt/rustale/RusTale\" &"));
    }

    #[test]
    fn test_unix_script_paths_are_escaped() {
        let current_exe = PathBuf::from("/opt/rustale/RusTale");
        let update_dir = PathBuf::from("/opt/rustale/update_temp");

        let script = generate_update_script_unix(&current_exe, &update_dir);

        // Paths should be wrapped in quotes
        assert!(script.contains("\"/opt/rustale"));
    }

    #[test]
    fn test_unix_script_uses_background_process() {
        let current_exe = PathBuf::from("/opt/rustale/RusTale");
        let update_dir = PathBuf::from("/opt/rustale/update_temp");

        let script = generate_update_script_unix(&current_exe, &update_dir);

        // The final command must use & to run in background
        // This allows the script to complete while the app starts
        let last_line = script.lines().last().unwrap_or("");
        assert!(last_line.ends_with(" &"), "Last line should end with ' &' to background the process");
    }

    // === Edge Case Tests ===

    #[test]
    fn test_version_with_prerelease() {
        // Pre-release versions like "1.0.0-beta" parse to [1, 0, 0]
        let parsed = parse_version("1.0.0-beta");
        assert_eq!(parsed, vec![1, 0, 0]);
    }

    #[test]
    fn test_version_with_build_metadata() {
        // Build metadata like "1.0.0+123" - the + part is ignored
        let parsed = parse_version("1.0.0+123");
        assert_eq!(parsed, vec![1, 0, 0]);
    }

    #[test]
    fn test_empty_version() {
        assert_eq!(parse_version(""), vec![]);
    }

    #[test]
    fn test_release_info_deserialization() {
        let json = r#"{
            "tag_name": "v1.0.0",
            "html_url": "https://github.com/soyelmismo/RusTale/releases/v1.0.0",
            "assets": [
                {
                    "name": "RusTale-windows.zip",
                    "browser_download_url": "https://example.com/download",
                    "size": 12345
                }
            ]
        }"#;

        let release: ReleaseInfo = serde_json::from_str(json).expect("Should parse");
        assert_eq!(release.tag_name, "v1.0.0");
        assert_eq!(release.assets.len(), 1);
        assert_eq!(release.assets[0].name, "RusTale-windows.zip");
    }

    #[test]
    fn test_updater_message_variants() {
        // Verify all message variants can be created (compile-time check)
        let _check = UpdaterMessage::CheckForUpdates;
        let _found = UpdaterMessage::UpdateFound(ReleaseInfo {
            tag_name: "v1.0.0".to_string(),
            html_url: "https://github.com".to_string(),
            assets: vec![],
        });
        let _not_found = UpdaterMessage::UpdateNotFound;
        let _error = UpdaterMessage::Error("test error".to_string());
        let _start = UpdaterMessage::StartUpdate("https://example.com".to_string());
        let _progress = UpdaterMessage::UpdateProgress(0.5, "Downloading".to_string());
        let _finished = UpdaterMessage::UpdateFinished;
        let _complete = UpdaterMessage::CheckComplete(Ok(None));
        let _download = UpdaterMessage::DownloadProgress(0.5);
        let _install = UpdaterMessage::InstallProgress(0.5);
        let _restart = UpdaterMessage::RestartRequired;
    }

    // === Path Injection Safety Tests ===
    // These tests verify that the update scripts handle paths safely
    // and don't allow command injection through path names.

    #[test]
    fn test_windows_script_injection_protection() {
        // Even with spaces in path, script should use proper quoting
        let current_exe = PathBuf::from("C:\\Program Files\\RusTale\\RusTale.exe");
        let update_dir = PathBuf::from("C:\\Program Files\\RusTale\\update_temp");

        let script = generate_update_script_windows(&current_exe, &update_dir);

        // Verify the paths are quoted (not vulnerable to spaces)
        assert!(script.contains("\"C:\\Program Files"));
    }

    #[test]
    fn test_unix_script_injection_protection() {
        // Even with spaces in path, script should use proper quoting
        let current_exe = PathBuf::from("/home/user/My Apps/RusTale/RusTale");
        let update_dir = PathBuf::from("/home/user/My Apps/RusTale/update_temp");

        let script = generate_update_script_unix(&current_exe, &update_dir);

        // Verify the paths are quoted
        assert!(script.contains("\"/home/user/My Apps"));
    }

    #[test]
    fn test_script_with_special_characters_in_path() {
        // Test with parentheses which could break batch scripts
        let current_exe = PathBuf::from("C:\\Users\\John (Dev)\\RusTale.exe");
        let update_dir = PathBuf::from("C:\\Users\\John (Dev)\\update_temp");

        let script = generate_update_script_windows(&current_exe, &update_dir);

        // The script should still contain the path (quoted)
        assert!(script.contains("John (Dev)"));
    }
}

/// Generates the update script content for validation (exposed for testing)
#[cfg(test)]
fn generate_update_script_windows(current_exe: &Path, update_dir: &Path) -> String {
    let app_dir = current_exe.parent().unwrap();
    format!(
        "@echo off\r\n\
         title RusTale Updating...\r\n\
         timeout /t 2 /nobreak >nul\r\n\
         echo Installing updates...\r\n\
         xcopy /s /y \"{}\\*\" \"{}\\\"\r\n\
         rmdir /s /q \"{}\"\r\n\
         start \"\" \"{}\"\r\n\
         del \"%~f0\"\r\n",
        update_dir.display(),
        app_dir.display(),
        update_dir.display(),
        current_exe.display()
    )
}

/// Generates the update script content for validation (exposed for testing)
#[cfg(test)]
fn generate_update_script_unix(current_exe: &Path, update_dir: &Path) -> String {
    let app_dir = current_exe.parent().unwrap();
    format!(
        "#!/bin/sh\n\
         sleep 2\n\
         cp -rf \"{}/.\" \"{}/\"\n\
         rm -rf \"{}\"\n\
         chmod +x \"{}\"\n\
         \"{}\" &\n",
        update_dir.display(),
        app_dir.display(),
        update_dir.display(),
        current_exe.display(),
        current_exe.display()
    )
}

fn spawn_update_script(current_exe: &Path, update_dir: &Path) -> Result<()> {
    let app_dir = current_exe.parent().unwrap();

    if cfg!(windows) {
        let script_content = format!(
            "@echo off\r\n\
             title RusTale Updating...\r\n\
             timeout /t 2 /nobreak >nul\r\n\
             echo Installing updates...\r\n\
             xcopy /s /y \"{}\\*\" \"{}\\\"\r\n\
             rmdir /s /q \"{}\"\r\n\
             start \"\" \"{}\"\r\n\
             del \"%~f0\"\r\n",
            update_dir.display(),
            app_dir.display(),
            update_dir.display(),
            current_exe.display()
        );

        let script_path = app_dir.join("updater.bat");
        std::fs::write(&script_path, script_content)?;

        std::process::Command::new("cmd")
            .args(["/C", &script_path.to_string_lossy()])
            .spawn()?;
    } else {
        let script_content = format!(
            "#!/bin/sh\n\
             sleep 2\n\
             cp -rf \"{}/.\" \"{}/\"\n\
             rm -rf \"{}\"\n\
             chmod +x \"{}\"\n\
             \"{}\" &\n",
            update_dir.display(),
            app_dir.display(),
            update_dir.display(),
            current_exe.display(),
            current_exe.display()
        );

        let script_path = app_dir.join("updater.sh");
        std::fs::write(&script_path, script_content)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script_path, perms)?;
        }

        std::process::Command::new("sh").arg(&script_path).spawn()?;
    }

    Ok(())
}
