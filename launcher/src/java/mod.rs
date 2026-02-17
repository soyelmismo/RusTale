use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};

use crate::game::patch_api::utils::*;

pub mod proxy;


/// Downloads and installs JRE if not already installed.
/// Installs into `.../RusTale/tools/jre/latest` to persist across game deletions.
/// Downloads JRE with automatic fallback using PatchApiManager
pub async fn download_jre(
    client: &reqwest::Client,
    base_dir: &PathBuf,
    progress_callback: impl Fn(&str, f64, &str, u64, u64, Option<String>) + Clone + Send + Sync + 'static,
    cancel_token: Option<Arc<AtomicBool>>,
) -> Result<()> {
    // Map our local callback to match the frontend signature
    let cb = move |phase: &str, pct: f64, msg: &str, total: u64, down: u64, eta: Option<String>| {
        progress_callback(phase, pct, msg, total, down, eta);
    };
    
    crate::game::patch_api::PatchApiFrontend::get_instance()
        .download_jre(client, base_dir, cb, cancel_token)
        .await
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

/// Checks if JRE is properly installed at the specified path
pub fn is_jre_installed_at(jre_path: &PathBuf) -> bool {
    if !jre_path.exists() {
        return false;
    }

    // Check for Java executable
    let java_exec = if cfg!(windows) {
        jre_path.join("bin").join("java.exe")
    } else {
        jre_path.join("bin").join("java")
    };

    java_exec.exists()
}
