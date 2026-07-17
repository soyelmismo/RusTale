//! Hytale Public Provider - Official Hytale API (no authentication required)
//!
//! This provider connects to the official Hytale servers for patch downloads.
//! It serves as a fallback when private mirrors are unavailable.
//!
//! Note: This endpoint may require authentication in the future.

use anyhow::Result;
use async_trait::async_trait;

#[cfg(feature = "security")]
use rustale_security::{init_shield, SafeString};

use crate::patch_api::traits::PatchProvider;
use crate::network::HTTP_CLIENT;

#[cfg(feature = "security")]
#[derive(serde::Deserialize, Debug)]
pub struct DeviceAuthResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: i32,
    pub interval: i32,
}

#[cfg(feature = "security")]
#[derive(serde::Deserialize, Debug)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub session_token: Option<String>,
    pub identity_token: Option<String>,
}

/// Official Hytale API provider (public endpoint)
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

    #[cfg(feature = "security")]
    pub fn get_session_token(&self) -> Option<SafeString> {
        rustale_security::get_private_var_opt("Z_Z_B")
    }

    #[cfg(feature = "security")]
    pub fn get_identity_token(&self) -> Option<SafeString> {
        rustale_security::get_private_var_opt("Z_Z_C")
    }

    #[cfg(feature = "security")]
    pub async fn initiate_device_auth(&self) -> anyhow::Result<DeviceAuthResponse> {
        let client_id_safe = rustale_security::require_private_var("Z_Z_A")?;
        
        let auth_url = rustale_security::require_private_var("Z_Z_D")?;
        let p_client_id = rustale_security::require_private_var("Z_Z_F")?;
        
        let params = [(p_client_id.as_str(), client_id_safe.as_str())];
        let res = HTTP_CLIENT
            .post(auth_url.as_str())
            .form(&params)
            .send()
            .await?;
            
        if !res.status().is_success() {
            anyhow::bail!("Failed to initiate device auth: {}", res.status());
        }
        
        Ok(res.json::<DeviceAuthResponse>().await?)
    }

    #[cfg(feature = "security")]
    pub async fn poll_device_token(&self, device_code: &str) -> anyhow::Result<TokenResponse> {
        let client_id_safe = rustale_security::require_private_var("Z_Z_A")?;
        
        let token_url = rustale_security::require_private_var("Z_Z_E")?;
        let p_client_id = rustale_security::require_private_var("Z_Z_F")?;
        let p_device_code = rustale_security::require_private_var("Z_Z_G")?;
        let p_grant_type = rustale_security::require_private_var("Z_Z_H")?;
        let v_grant_type = rustale_security::require_private_var("Z_Z_I")?;
        
        let params = [
            (p_client_id.as_str(), client_id_safe.as_str()),
            (p_device_code.as_str(), device_code),
            (p_grant_type.as_str(), v_grant_type.as_str()),
        ];
        
        let res = HTTP_CLIENT
            .post(token_url.as_str())
            .form(&params)
            .send()
            .await?;
            
        if !res.status().is_success() {
            anyhow::bail!("Failed to poll token: {}", res.status());
        }
        
        Ok(res.json::<TokenResponse>().await?)
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

        #[cfg(feature = "security")]
        let template = rustale_security::get_private_var("Z_Z_T").into_string();
        #[cfg(not(feature = "security"))]
        let template = String::new();
        
        if template.is_empty() {
            return false;
        }

        let url = template
            .replacen("{}", os, 1)
            .replacen("{}", arch, 1)
            .replacen("{}", channel, 1)
            .replacen("{}", &start_version.to_string(), 1)
            .replacen("{}", &end_version.to_string(), 1);

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
        #[cfg(feature = "security")]
        let url = rustale_security::get_private_var("Z_Z_J").into_string();
        #[cfg(not(feature = "security"))]
        let url = String::new();

        if url.is_empty() {
            return false;
        }

        match HTTP_CLIENT
            .get(&url)
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



    #[cfg(feature = "security")]
    async fn download_patch_secure(
        &self,
        channel: &str,
        os: &str,
        arch: &str,
        from_version: i32,
        to_version: i32,
        dest_path: &std::path::Path,
        cancel_token: std::sync::Arc<std::sync::atomic::AtomicBool>,
        progress_callback: Box<dyn Fn(f64, u64, u64) + Send + Sync>,
    ) -> Result<()> {
        let arch_str = match arch {
            "x86_64" => "amd64",
            "aarch64" => "arm64",
            _ => arch,
        };

        let os_str = match os {
            "darwin" => "mac",
            _ => os,
        };

        let template = rustale_security::require_private_var("Z_Z_T")?.into_string();

        let url = template
            .replacen("{}", os_str, 1)
            .replacen("{}", arch_str, 1)
            .replacen("{}", channel, 1)
            .replacen("{}", &from_version.to_string(), 1)
            .replacen("{}", &to_version.to_string(), 1);

        crate::network::download_file(
            &url,
            dest_path,
            |_, pct, _, total, downloaded, _, _| {
                progress_callback(pct, downloaded, total);
            },
            Some(cancel_token),
        ).await
    }
}

impl Default for HytaleProvider {
    fn default() -> Self {
        Self::new()
    }
}