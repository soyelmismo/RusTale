use anyhow::Result;
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};
use crate::ProgressCallback;
use super::PatchApiFrontend;

impl PatchApiFrontend {
    /// Installs Butler using the new patch API system
    /// Replaces: crate::game::patcher::install_butler
    pub async fn install_butler(
        &self,
        base_dir: &PathBuf,
        progress_callback: ProgressCallback,
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> Result<PathBuf> {
        self.butler_installer
            .install(base_dir, progress_callback, cancel_token, &self.localization)
            .await
    }

    /// Downloads and installs JRE using the new patch API system
    /// Replaces: crate::java::download_jre
    pub async fn download_jre(
        &self,
        base_dir: &PathBuf,
        progress_callback: ProgressCallback,
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> Result<()> {
        self.jre_installer
            .install(base_dir, progress_callback, cancel_token, &self.localization)
            .await
    }
}