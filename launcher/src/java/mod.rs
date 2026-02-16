use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};

pub mod proxy;

#[derive(Debug, serde::Deserialize)]
struct JREPlatform {
    url: String,
}

#[derive(Debug, serde::Deserialize, Default)]
struct JREJSON {
    download_url: std::collections::HashMap<String, std::collections::HashMap<String, JREPlatform>>,
}

/// Downloads and installs JRE if not already installed.
/// Installs into `.../RusTale/tools/jre/latest` to persist across game deletions.
/// Downloads JRE with automatic fallback using PatchApiManager
pub async fn download_jre(
    client: &reqwest::Client,
    base_dir: &PathBuf,
    progress_callback: impl Fn(&str, f64, &str, u64, u64, Option<String>),
    cancel_token: Option<Arc<AtomicBool>>,
) -> Result<()> {
    // Use PatchApiManager to get JRE URL with automatic fallback
    let manager = crate::game::patch_api::PatchApiManager::new();
    let jre_url = manager.get_jre_url(std::env::consts::OS, get_arch_name()).await
        .context("Failed to get JRE URL from all providers")?;
    
    println!("Using JRE URL: {}", jre_url);
    
    // Create a simple JREJSON structure for the download function
    let mut jre_data = JREJSON::default();
    jre_data.download_url.insert(
        get_os_name().to_string(),
        std::collections::HashMap::from([(
            get_arch_name().to_string(),
            JREPlatform { url: jre_url }
        )])
    );
    
    download_jre_from_data(client, &jre_data, base_dir, &progress_callback, cancel_token).await
}

/// Downloads JRE from parsed data
async fn download_jre_from_data(
    client: &reqwest::Client,
    jre_data: &JREJSON,
    base_dir: &PathBuf,
    progress_callback: &impl Fn(&str, f64, &str, u64, u64, Option<String>),
    cancel_token: Option<Arc<AtomicBool>>,
) -> anyhow::Result<()> {
    let os_name = get_os_name();
    let arch = get_arch_name();

    let os_data = jre_data
        .download_url
        .get(os_name)
        .context(format!("No JRE available for OS: {}", os_name))?;

    let platform = os_data.get(arch).context(format!(
        "No JRE available for arch: {} on {}",
        arch, os_name
    ))?;

    let paths = crate::game::paths::GamePaths::new(base_dir.clone());
    let jre_base_dir = paths.tools().join("jre");
    let latest_dir = jre_base_dir.join("latest");
    let cache_dir = crate::config::get_cache_dir("jre").await;
    tokio::fs::create_dir_all(&jre_base_dir).await?;
    tokio::fs::create_dir_all(&cache_dir).await?;

    let file_name = platform.url.split('/').last().unwrap_or("jre.zip");
    let cache_file = cache_dir.join(file_name);

    // Download if not cached
    if !cache_file.exists() {
        progress_callback("jre", 10.0, &format!("Downloading {}...", file_name), 0, 0, None);

        crate::game::downloader::download_file(
            client,
            &platform.url,
            &cache_file,
            |pct, speed, total, downloaded, eta| {
                let size_info = if total > 0 {
                    format!("{} / {}", 
                        crate::game::downloader::format_bytes(downloaded), 
                        crate::game::downloader::format_bytes(total)
                    )
                } else {
                    crate::game::downloader::format_bytes(downloaded)
                };
                
                let eta_info = if let Some(eta_str) = &eta {
                    format!(" • ETA: {}", eta_str)
                } else {
                    String::new()
                };
                
                progress_callback(
                    "jre",
                    pct as f64,
                    &format!("Downloading JRE... ({}{}{})", speed, size_info, eta_info),
                    total,
                    downloaded,
                    eta,
                );
            },
            cancel_token,
        )
        .await?;
    }

    progress_callback("jre", 70.0, "Extracting JRE...", 0, 0, None);

    // Only clean up if the directory exists but doesn't contain a valid JRE
    let should_clean = if latest_dir.exists() {
        !crate::java::is_jre_installed_at(&latest_dir)
    } else {
        true // Directory doesn't exist, we need to create it
    };

    if should_clean {
        if latest_dir.exists() {
            tokio::fs::remove_dir_all(&latest_dir).await?;
        }
        tokio::fs::create_dir_all(&latest_dir).await?;
    }

    // Extract using spawn_blocking to avoid UI freeze
    let cache_file_clone = cache_file.clone();
    let latest_dir_clone = latest_dir.clone();

    tokio::task::spawn_blocking(move || -> Result<()> {
        extract_archive(&cache_file_clone, &latest_dir_clone)?;
        Ok(())
    })
    .await
    .context("JRE extraction task failed")??;

    progress_callback("jre", 100.0, "JRE installed", 0, 0, None);

    Ok(())
}

/// Gets the path to the Java executable from the tools/jre/latest directory
pub fn get_java_exec(base_dir: &PathBuf) -> Result<String> {
    let paths = crate::game::paths::GamePaths::new(base_dir.clone());
    let java_bin = paths.java_exec();

    if !java_bin.exists() {
        anyhow::bail!("JRE executable not found at: {}", java_bin.display());
    }

    java_bin
        .to_str()
        .map(|s| s.to_string())
        .context("Invalid Java path encoding")
}

pub fn is_jre_installed_at(jre_dir: &PathBuf) -> bool {
    // We already have the dir, but we can't easily use GamePaths here
    // without potentially changing the logic (is_jre_installed_at is called with latest_dir).
    // However, if we want to use the unified paths:
    let java_bin = if cfg!(windows) {
        jre_dir.join("bin").join("java.exe")
    } else {
        jre_dir.join("bin").join("java")
    };
    
    println!("[JRE Debug] Checking for Java at: {}", java_bin.display());
    let exists = java_bin.exists();
    println!("[JRE Debug] Java exists: {}", exists);
    
    exists
}

fn get_os_name() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "darwin",
        "linux" => "linux",
        other => other,
    }
}

fn get_arch_name() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    }
}

fn flatten_jre_directory(dest_dir: &PathBuf) -> Result<()> {
    // Check if extraction created a subdirectory (common with JRE distributions)
    // and move its contents up one level
    if let Ok(entries) = std::fs::read_dir(dest_dir) {
        let subdirs: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
            
        // If there's exactly one subdirectory and it looks like a JRE directory
        if subdirs.len() == 1 {
            let subdir = &subdirs[0];
            let subdir_name = subdir.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
                
            // Common JRE directory patterns
            if subdir_name.starts_with("jdk-") || 
               subdir_name.contains("jre") || 
               subdir_name.starts_with("java-") {
                
                println!("[JRE] Moving contents from subdirectory: {}", subdir_name);
                
                // Move all contents from subdirectory to dest_dir
                if let Ok(entries) = std::fs::read_dir(subdir) {
                    for entry in entries.flatten() {
                        let src_path = entry.path();
                        let dest_path = dest_dir.join(entry.file_name());
                        
                        if src_path.is_file() {
                            std::fs::rename(&src_path, &dest_path)
                                .context(format!("Failed to move file {:?}", src_path))?;
                        } else if src_path.is_dir() {
                            std::fs::rename(&src_path, &dest_path)
                                .context(format!("Failed to move directory {:?}", src_path))?;
                        }
                    }
                }
                
                // Remove the now-empty subdirectory
                std::fs::remove_dir_all(subdir)
                    .context(format!("Failed to remove subdirectory {:?}", subdir))?;
                    
                println!("[JRE] Successfully flattened JRE directory structure");
            }
        }
    }
    Ok(())
}

fn extract_archive(archive_path: &PathBuf, dest_dir: &PathBuf) -> Result<()> {
    std::fs::create_dir_all(dest_dir)?;

    let file = std::fs::File::open(archive_path).context("Failed to open archive")?;

    if archive_path.to_string_lossy().ends_with(".zip") {
        let mut archive = zip::ZipArchive::new(file).context("Failed to read ZIP archive")?;
        archive.extract(dest_dir).context("Failed to extract ZIP")?;
        flatten_jre_directory(dest_dir)?;
    } else if archive_path.to_string_lossy().ends_with(".tar.gz") {
        let gz = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(gz);
        archive
            .unpack(dest_dir)
            .context("Failed to extract tar.gz")?;
        flatten_jre_directory(dest_dir)?;
    } else {
        anyhow::bail!("Unsupported archive format");
    }
    Ok(())
}


