use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};

use crate::patch_api::utils::{get_java_adoptium_url, format_bytes};
use crate::paths::GamePaths;
use crate::java::is_jre_installed_at;
use crate::ProgressCallback;

/// Installer for JRE
#[derive(Clone)]
pub struct JreInstaller {}

impl JreInstaller {
    pub fn new() -> Self {
        Self {}
    }

    /// Downloads and installs JRE
    pub async fn install(
        &self,
        base_dir: &PathBuf,
        progress_callback: ProgressCallback,
        cancel_token: Option<Arc<AtomicBool>>,
        localization: &crate::lang::Localization,
    ) -> Result<()> {
        let os = std::env::consts::OS;
        let arch = crate::patch_api::utils::get_arch_name();

        let url = get_java_adoptium_url(os, arch);

        self.download_jre_from_url(&url, base_dir, progress_callback.clone(), cancel_token, localization)
            .await
    }

    /// Downloads JRE from a specific URL
    async fn download_jre_from_url(
        &self,
        jre_url: &str,
        base_dir: &PathBuf,
        progress_callback: ProgressCallback,
        cancel_token: Option<Arc<AtomicBool>>,
        localization: &crate::lang::Localization,
    ) -> Result<()> {
        let paths = GamePaths::new(base_dir.clone());
        let jre_base_dir = paths.tools().join("jre");
        let latest_dir = jre_base_dir.join("latest");
        let cache_dir = crate::config::get_cache_dir("jre").await;
        tokio::fs::create_dir_all(&jre_base_dir).await?;
        tokio::fs::create_dir_all(&cache_dir).await?;

        // FIX: Force extension based on OS to ensure extractor recognizes it
        let extension = if cfg!(windows) { "zip" } else { "tar.gz" };
        let file_name = format!("jre_runtime.{}", extension);
        let cache_file = cache_dir.join(&file_name);

        if cache_file.exists() {
            // Basic size check to prevent corrupt partial downloads being used
            if let Ok(meta) = std::fs::metadata(&cache_file) {
                if meta.len() < 1_000_000 { // < 1MB is suspicious for JRE
                     let _ = tokio::fs::remove_file(&cache_file).await;
                }
            }
        }

        if !cache_file.exists() {
            progress_callback(
                "jre".to_string(),
                10.0,
                format!("Downloading {}...", file_name),
                0,
                0,
                None,
                None,
            );

            let progress_callback_clone = progress_callback.clone();
            crate::download_file(
                jre_url,
                &cache_file,
                move |_phase, pct, speed, total, downloaded, eta, _step| {
                    let size_info = if total > 0 {
                        format!(
                            "{} / {}",
                            format_bytes(downloaded),
                            format_bytes(total)
                        )
                    } else {
                        format_bytes(downloaded)
                    };

                    let eta_info = if let Some(eta_str) = &eta {
                        format!(" • ETA: {}", eta_str)
                    } else {
                        String::new()
                    };

                    progress_callback_clone(
                        "jre".to_string(),
                        pct,
                        format!("Downloading JRE... ({}{}{})", speed, size_info, eta_info),
                        total,
                        downloaded,
                        eta,
                        None,
                    );
                },
                cancel_token,
            )
            .await?;
        }

        progress_callback("jre".to_string(), 70.0, "Extracting JRE...".to_string(), 0, 0, None, None);

        let should_clean = if latest_dir.exists() {
            !is_jre_installed_at(&latest_dir)
        } else {
            true 
        };

        if should_clean {
            if latest_dir.exists() {
                tokio::fs::remove_dir_all(&latest_dir).await?;
            }
            tokio::fs::create_dir_all(&latest_dir).await?;
        }

        let cache_file_clone = cache_file.clone();
        let latest_dir_clone = latest_dir.clone();

        tokio::task::spawn_blocking(move || -> Result<()> {
            extract_archive(&cache_file_clone, &latest_dir_clone)?;
            Ok(())
        })
        .await
        .context("JRE extraction task failed")??;

        progress_callback("jre".to_string(), 100.0, localization.t("common.jre_installed").to_string(), 0, 0, None, None);

        Ok(())
    }
}

/// Extract archive (ZIP or tar.gz) and flatten if necessary
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

/// Flatten JRE directory if extraction created a subdirectory
fn flatten_jre_directory(dest_dir: &PathBuf) -> Result<()> {
    if let Ok(entries) = std::fs::read_dir(dest_dir) {
        let subdirs: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();

        if subdirs.len() == 1 {
            let subdir = &subdirs[0];
            let subdir_name = subdir.file_name().and_then(|n| n.to_str()).unwrap_or("");

            if subdir_name.starts_with("jdk-")
                || subdir_name.contains("jre")
                || subdir_name.starts_with("java-")
            {
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

                std::fs::remove_dir_all(subdir)
                    .context(format!("Failed to remove subdirectory {:?}", subdir))?;
            }
        }
    }
    Ok(())
}
