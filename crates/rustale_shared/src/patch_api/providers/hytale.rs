//! Hytale Public Provider - Official Hytale API (no authentication required)
//!
//! This provider connects to the official Hytale servers for patch downloads.
//! It serves as a fallback when private mirrors are unavailable.
//!
//! Note: This endpoint may require authentication in the future.

use anyhow::Result;
use async_trait::async_trait;

#[cfg(feature = "security")]
use rustale_security::init_shield;

use crate::patch_api::traits::PatchProvider;
use crate::network::HTTP_CLIENT;

/// Official Hytale API provider (public endpoint)
///
/// This is a public provider that doesn't require authentication.
/// Uses the standard HTTP_CLIENT from the network module.
pub struct HytaleProvider {
    #[cfg(feature = "security")]
    _initialized: bool,
}

impl HytaleProvider {
    pub fn new() -> Self {
        #[cfg(feature = "security")]
        {
            init_shield();
            Self { _initialized: true }
        }
        #[cfg(not(feature = "security"))]
        Self {}
    }

    /// Check if a patch version exists on the server
    pub async fn check_version_exists(
        &self,
        start_version: i32,
        end_version: i32,
        architecture: &str,
        operating_system: &str,
        channel: &str,
    ) -> bool {
        let arch = match architecture {
            "x86_64" => "amd64",
            "aarch64" => "arm64",
            _ => architecture,
        };

        let os = match operating_system {
            "darwin" => "mac",
            _ => operating_system,
        };

        let url = format!(
            "https://account-data.hytale.com/patches/{}/{}/{}/{}/{}.pwr",
            os, arch, channel, start_version, end_version
        );

        for attempt in 0..3 {
            match HTTP_CLIENT.head(&url).send().await {
                Ok(resp) => {
                    match resp.status().as_u16() {
                        200 => return true,
                        404 => return false,
                        _ => {
                            if attempt < 2 {
                                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                            }
                        }
                    }
                }
                Err(_) => {
                    if attempt < 2 {
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    }
                }
            }
        }
        false
    }


}

#[async_trait]
impl PatchProvider for HytaleProvider {
    fn name(&self) -> &str {
        "hytale-official"
    }

    fn priority(&self) -> i32 {
        10 // Low priority - public fallback only
    }

    async fn is_available(&self) -> bool {
        // Simple connectivity check to official servers
        match HTTP_CLIENT
            .get("https://launcher.hytale.com/version/release/launcher.json")
            .send()
            .await
        {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    async fn get_latest_version(&self, channel: &str, os: &str, arch: &str) -> Result<i32> {
        // Exponential search for latest version
        let mut last_found = 0;
        let mut next_check = 1;
        let mut step = 2;

        while next_check <= 100 {
            let exists = self
                .check_version_exists(0, next_check, arch, os, channel)
                .await;

            if exists {
                last_found = next_check;
                next_check += step;
                step += 1;
            } else {
                break;
            }
        }

        if last_found == 0 {
            anyhow::bail!("Hytale official servers are unreachable");
        }

        // Binary search between last_found and next_check
        let mut low = last_found;
        let mut high = next_check - 1;
        let mut result = last_found;

        while low <= high {
            let mid = (low + high) / 2;
            if mid <= result {
                low = mid + 1;
                continue;
            }

            let exists = self.check_version_exists(0, mid, arch, os, channel).await;

            if exists {
                result = mid;
                low = mid + 1;
            } else {
                high = mid - 1;
            }
        }

        Ok(result)
    }

    async fn get_available_versions(
        &self,
        channel: &str,
        os: &str,
        arch: &str,
    ) -> Result<Vec<i32>> {
        let latest = self.get_latest_version(channel, os, arch).await?;
        let mut versions = Vec::new();

        // Use milestones instead of checking every version
        let milestones = [1, 3, 6, 10, 15, 20, 30, 50, 75, 100];
        
        for &v in &milestones {
            if v <= latest && self.check_version_exists(0, v, arch, os, channel).await {
                versions.push(v);
            }
        }

        // Always include latest
        if latest > 0 && self.check_version_exists(0, latest, arch, os, channel).await {
            versions.push(latest);
        }

        versions.sort();
        versions.dedup();

        Ok(versions)
    }



    /// HytaleProvider no soporta descarga segura directa.
    /// Usar get_patch_url() + download_file() en su lugar.
    #[cfg(feature = "security")]
    async fn download_patch_secure(
        &self,
        _channel: &str,
        _os: &str,
        _arch: &str,
        _from_version: i32,
        _to_version: i32,
        _dest_path: &std::path::Path,
        _cancel_token: std::sync::Arc<std::sync::atomic::AtomicBool>,
        _progress_callback: Box<dyn Fn(f64, u64, u64) + Send + Sync>,
    ) -> Result<()> {
        anyhow::bail!("HytaleProvider does not support secure direct download. Use get_patch_url() + download_file() instead.")
    }
}

impl Default for HytaleProvider {
    fn default() -> Self {
        Self::new()
    }
}