use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};
use tokio::io::AsyncBufReadExt;

use crate::game::paths::GamePaths;
use crate::util::make_executable;
use crate::game::patch_api::get_butler_fallback_url;

/// Installer for Butler using the new patch API system
#[derive(Clone)]
pub struct ButlerInstaller {
}

impl ButlerInstaller {
    pub fn new() -> Self {
        Self {}
    }

    /// Installs Butler if not already installed
    pub async fn install(
        &self,
        client: &reqwest::Client,
        base_dir: &PathBuf,
        progress_callback: impl Fn(&str, f64, &str, u64, u64, Option<String>, Option<usize>),
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> Result<PathBuf> {
        let paths = GamePaths::new(base_dir.clone());
        let tools_dir = base_dir.join("tools").join("butler");
        let butler_path = paths.butler();
        tokio::fs::create_dir_all(&tools_dir).await?;

        // Check if already installed
        if butler_path.exists() {
            let _ = make_executable(&butler_path).await;
            progress_callback("butler", 100.0, "Butler already installed", 0, 0, None, None);
            return Ok(butler_path);
        }

        // Get Butler URL using the patch API system
        let os = std::env::consts::OS;
        let arch = crate::game::patch_api::utils::get_arch_name();

        let url = get_butler_fallback_url(os, arch);

        progress_callback("butler", 0.0, "Downloading Butler...", 0, 0, None, None);

        let zip_path = tools_dir.join("butler.zip");

        // Validate cached file before using it
        if zip_path.exists() {
            let file_name = url.split('/').last().unwrap_or("butler.zip");
            if !crate::game::patch_api::utils::looks_like_butler_file(file_name) {
                progress_callback("butler", 5.0, &format!("Invalid cached file: {}", file_name), 0, 0, None, None);
                tokio::fs::remove_file(&zip_path).await?;
            } else {
                // Cached file is valid, proceed to extraction
                progress_callback("butler", 70.0, "Extracting Butler...", 0, 0, None, None);
                
                let zip_path_clone = zip_path.clone();
                let tools_dir_clone = tools_dir.clone();
                
                tokio::task::spawn_blocking(move || -> Result<()> {
                    let file = std::fs::File::open(&zip_path_clone)
                        .context("Failed to open Butler archive")?;
                    let mut archive = zip::ZipArchive::new(file)
                        .context("Failed to read Butler archive")?;
                    archive
                        .extract(&tools_dir_clone)
                        .context("Failed to extract Butler")?;
                    Ok(())
                })
                .await
                .context("Butler extraction task failed")??;
                
                // Make executable on Unix
                let _ = make_executable(&butler_path).await;
                
                progress_callback("butler", 100.0, "Butler installed from cache", 0, 0, None, None);
                
                return Ok(butler_path);
            }
        }

        // Download using the existing downloader
        crate::game::downloader::download_file(
            client,
            &url,
            &zip_path,
            |pct, speed, total, downloaded, eta| {
                progress_callback(
                    "butler", 
                    pct as f64, 
                    &format!("Downloading Butler... ({})", speed), 
                    total, 
                    downloaded, 
                    eta, 
                    None
                );
            },
            cancel_token,
        ).await?;

        progress_callback("butler", 70.0, "Extracting Butler...", 0, 0, None, None);

        // Extract using spawn_blocking to avoid UI freeze
        let zip_path_clone = zip_path.clone();
        let tools_dir_clone = tools_dir.clone();

        tokio::task::spawn_blocking(move || -> Result<()> {
            let file = std::fs::File::open(&zip_path_clone)
                .context("Failed to open Butler archive")?;
            let mut archive = zip::ZipArchive::new(file)
                .context("Failed to read Butler archive")?;
            archive
                .extract(&tools_dir_clone)
                .context("Failed to extract Butler")?;
            Ok(())
        })
        .await
        .context("Butler extraction task failed")??;

        // Make executable on Unix
        let _ = make_executable(&butler_path).await;

        // Cleanup
        let _ = tokio::fs::remove_file(&zip_path).await;

        progress_callback("butler", 100.0, "Butler installed", 0, 0, None, None);

        Ok(butler_path)
    }
}

/// Installer for JRE using the new patch API system
#[derive(Clone)]
pub struct JreInstaller {
}

impl JreInstaller {
    pub fn new() -> Self {
        Self {}
    }

    /// Downloads and installs JRE if not already installed
    pub async fn install(
        &self,
        client: &reqwest::Client,
        base_dir: &PathBuf,
        progress_callback: impl Fn(&str, f64, &str, u64, u64, Option<String>),
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> Result<()> {
        // Get JRE URL using the patch API system
        let os = std::env::consts::OS;
        let arch = crate::game::patch_api::utils::get_arch_name();

        let url = crate::game::patch_api::utils::get_java_adoptium_url(os, arch);

        self.download_jre_from_url(client, &url, base_dir, &progress_callback, cancel_token).await
    }

    /// Downloads JRE from a specific URL
    async fn download_jre_from_url(
        &self,
        client: &reqwest::Client,
        jre_url: &str,
        base_dir: &PathBuf,
        progress_callback: &impl Fn(&str, f64, &str, u64, u64, Option<String>),
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> Result<()> {
        let paths = GamePaths::new(base_dir.clone());
        let jre_base_dir = paths.tools().join("jre");
        let latest_dir = jre_base_dir.join("latest");
        let cache_dir = crate::config::get_cache_dir("jre").await;
        tokio::fs::create_dir_all(&jre_base_dir).await?;
        tokio::fs::create_dir_all(&cache_dir).await?;

        let file_name = jre_url.split('/').last().unwrap_or("jre.zip");
        let cache_file = cache_dir.join(file_name);

        // Validate cached file before using it
        if cache_file.exists() {
            if !crate::game::patch_api::utils::looks_like_jre_file(file_name) {
                progress_callback("jre", 5.0, &format!("Invalid cached file: {}", file_name), 0, 0, None);
                tokio::fs::remove_file(&cache_file).await?;
            }
        }

        // Download if not cached
        if !cache_file.exists() {
            progress_callback("jre", 10.0, &format!("Downloading {}...", file_name), 0, 0, None);

            crate::game::downloader::download_file(
                client,
                jre_url,
                &cache_file,
                |pct, speed, total, downloaded, eta| {
                    let size_info = if total > 0 {
                        format!("{} / {}", 
                            crate::game::patch_api::utils::format_bytes(downloaded), 
                            crate::game::patch_api::utils::format_bytes(total)
                        )
                    } else {
                        crate::game::patch_api::utils::format_bytes(downloaded)
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
            ).await?;
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
}

/// Extract archive (ZIP or tar.gz) and flatten if necessary
fn extract_archive(archive_path: &PathBuf, dest_dir: &PathBuf) -> Result<()> {
    std::fs::create_dir_all(dest_dir)?;

    let file = std::fs::File::open(archive_path)
        .context("Failed to open archive")?;

    if archive_path.to_string_lossy().ends_with(".zip") {
        let mut archive = zip::ZipArchive::new(file)
            .context("Failed to read ZIP archive")?;
        archive.extract(dest_dir)
            .context("Failed to extract ZIP")?;
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

/// Flatten JRE directory if extraction created a subdirectory
fn flatten_jre_directory(dest_dir: &PathBuf) -> Result<()> {
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
