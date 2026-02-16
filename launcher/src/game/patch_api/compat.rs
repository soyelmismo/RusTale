//! Compatibility layer for existing code to use the new patch API system
//! This module provides drop-in replacements for existing functions

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};

use super::PatchApiFrontend;

/// Global instance of the patch API frontend
static mut PATCH_API_FRONTEND: Option<PatchApiFrontend> = None;
static INIT: std::sync::Once = std::sync::Once::new();

/// Initialize the global patch API frontend
pub fn init_patch_api() {
    INIT.call_once(|| {
        unsafe {
            PATCH_API_FRONTEND = Some(PatchApiFrontend::new());
        }
    });
}

/// Get the global patch API frontend instance
fn get_patch_api() -> &'static PatchApiFrontend {
    init_patch_api();
    unsafe { PATCH_API_FRONTEND.as_ref().unwrap() }
}

/// Drop-in replacement for crate::game::patcher::install_butler
pub async fn install_butler(
    client: &reqwest::Client,
    base_dir: &PathBuf,
    progress_callback: impl Fn(&str, f64, &str, u64, u64, Option<String>, Option<usize>),
    cancel_token: Option<Arc<AtomicBool>>,
) -> Result<PathBuf> {
    get_patch_api().install_butler(client, base_dir, progress_callback, cancel_token).await
}

/// Drop-in replacement for crate::java::download_jre
pub async fn download_jre(
    client: &reqwest::Client,
    base_dir: &PathBuf,
    progress_callback: impl Fn(&str, f64, &str, u64, u64, Option<String>),
    cancel_token: Option<Arc<AtomicBool>>,
) -> Result<()> {
    get_patch_api().download_jre(client, base_dir, progress_callback, cancel_token).await
}

/// Drop-in replacement for crate::game::ensure_installed
pub async fn ensure_installed(
    client: &reqwest::Client,
    base_dir: &PathBuf,
    channel: &str,
    target_version: Option<i32>,
    policy: crate::game::install::InstallPolicy,
    progress_callback: impl Fn(&str, f64, &str, u64, u64, Option<String>, Option<usize>),
    cancel_token: Option<Arc<AtomicBool>>,
) -> Result<(usize, ())> {
    get_patch_api().ensure_installed(client, base_dir, channel, target_version, policy, progress_callback, cancel_token).await
}

/// Drop-in replacement for crate::game::patcher::get_version_manifest
pub async fn get_version_manifest(
    client: &reqwest::Client,
    channel: &str,
    base_dir: &PathBuf,
    user_version: i32,
) -> Result<crate::game::patcher::GameVersionInfo> {
    get_patch_api().get_version_info(client, base_dir, channel, user_version).await
}

/// Drop-in replacement for crate::game::patcher::find_latest_version
pub async fn find_latest_version(
    channel: &str,
    start_hint: Option<i32>,
) -> Result<i32> {
    // Create a client for this operation
    let client = reqwest::Client::new();
    get_patch_api().find_latest_version(channel, start_hint).await
}

/// Enhanced version with client parameter for better performance
pub async fn find_latest_version_with_client(
    client: &reqwest::Client,
    channel: &str,
    start_hint: Option<i32>,
) -> Result<i32> {
    get_patch_api().find_latest_version(channel, start_hint).await
}

/// Drop-in replacement for crate::game::patcher::download_pwr
pub async fn download_pwr(
    client: &reqwest::Client,
    channel: &str,
    from_version: i32,
    to_version: i32,
    progress_callback: impl Fn(&str, f64, &str, u64, u64, Option<String>, Option<usize>),
    cancel_token: Option<Arc<AtomicBool>>,
) -> Result<PathBuf> {
    let base_dir = crate::config::get_app_dir();
    let downloader = get_patch_api().get_patch_downloader();
    downloader.download_patch(client, &base_dir, channel, from_version, to_version, progress_callback, cancel_token).await
}

/// Get the patch API frontend instance for advanced usage
pub fn get_patch_api_frontend() -> &'static PatchApiFrontend {
    get_patch_api()
}
