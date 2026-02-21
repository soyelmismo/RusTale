use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};

use crate::patch_api::utils::{get_butler_fallback_url, make_executable, looks_like_butler_file};
use crate::paths::GamePaths;
use crate::ProgressCallback;

/// Installer for Butler
#[derive(Clone)]
pub struct ButlerInstaller {}

impl ButlerInstaller {
    pub fn new() -> Self {
        Self {}
    }

    /// Installs Butler if not already installed
    pub async fn install(
        &self,
        base_dir: &PathBuf,
        progress_callback: ProgressCallback,
        cancel_token: Option<Arc<AtomicBool>>,
        localization: &crate::lang::Localization,
    ) -> Result<PathBuf> {
        let paths = GamePaths::new(base_dir.clone());
        let tools_dir = base_dir.join("tools").join("butler");
        let butler_path = paths.butler();
        tokio::fs::create_dir_all(&tools_dir).await?;

        // Check if already installed
        if butler_path.exists() {
            let _ = make_executable(&butler_path).await;
            progress_callback(
                "butler".to_string(),
                100.0,
                "Butler already installed".to_string(),
                0,
                0,
                None,
                None,
            );
            return Ok(butler_path);
        }

        let os = std::env::consts::OS;
        let arch = crate::patch_api::utils::get_arch_name();

        let url = get_butler_fallback_url(os, arch);

        progress_callback("butler".to_string(), 0.0, "Downloading Butler...".to_string(), 0, 0, None, None);

        let zip_path = tools_dir.join("butler.zip");

        if zip_path.exists() {
            let file_name = url.split('/').last().unwrap_or("butler.zip");
            if !looks_like_butler_file(file_name) {
                tokio::fs::remove_file(&zip_path).await?;
            } else {
                progress_callback("butler".to_string(), 70.0, "Extracting Butler...".to_string(), 0, 0, None, None);

                let zip_path_clone = zip_path.clone();
                let tools_dir_clone = tools_dir.clone();

                tokio::task::spawn_blocking(move || -> Result<()> {
                    let file = std::fs::File::open(&zip_path_clone)
                        .context("Failed to open Butler archive")?;
                    let mut archive =
                        zip::ZipArchive::new(file).context("Failed to read Butler archive")?;
                    archive
                        .extract(&tools_dir_clone)
                        .context("Failed to extract Butler")?;
                    Ok(())
                })
                .await
                .context("Butler extraction task failed")??;

                let _ = make_executable(&butler_path).await;

                progress_callback(
                    "butler".to_string(),
                    100.0,
                    localization.t("common.butler_installed").to_string(),
                    0,
                    0,
                    None,
                    None,
                );

                return Ok(butler_path);
            }
        }

        let progress_callback_clone = progress_callback.clone();
        crate::download_file(
            &url,
            &zip_path,
            |_phase, pct, speed, total, downloaded, eta, _step| {
                progress_callback_clone(
                    "butler".to_string(),
                    pct,
                    format!("Downloading Butler... ({})", speed),
                    total,
                    downloaded,
                    eta,
                    None,
                );
            },
            cancel_token,
        )
        .await?;

        progress_callback("butler".to_string(), 70.0, "Extracting Butler...".to_string(), 0, 0, None, None);

        let zip_path_clone = zip_path.clone();
        let tools_dir_clone = tools_dir.clone();

        tokio::task::spawn_blocking(move || -> Result<()> {
            let file =
                std::fs::File::open(&zip_path_clone).context("Failed to open Butler archive")?;
            let mut archive =
                zip::ZipArchive::new(file).context("Failed to read Butler archive")?;
            archive
                .extract(&tools_dir_clone)
                .context("Failed to extract Butler")?;
            Ok(())
        })
        .await
        .context("Butler extraction task failed")??;

        let _ = make_executable(&butler_path).await;
        let _ = tokio::fs::remove_file(&zip_path).await;

        progress_callback("butler".to_string(), 100.0, localization.t("common.butler_installed").to_string(), 0, 0, None, None);

        Ok(butler_path)
    }
}
