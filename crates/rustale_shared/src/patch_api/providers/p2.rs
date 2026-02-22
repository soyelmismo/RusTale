//! Provider2 - Non-Cloudflare mirror

use anyhow::Result;
use async_trait::async_trait;
use zeroize::Zeroizing;

#[cfg(feature = "security")]
use rustale_security::RawSecureClient;

use crate::patch_api::traits::PatchProvider;
#[cfg(feature = "security")]
use crate::patch_api::utils::{get_pinned_cert_hash, get_private_var};

/// Provider2 - mirror
#[cfg(feature = "security")]
pub struct Provider2 {
    raw_client: RawSecureClient,
}

#[cfg(feature = "security")]
impl Provider2 {
    pub fn new() -> Self {
        Self {
            raw_client: RawSecureClient::new(get_pinned_cert_hash),
        }
    }

    async fn check_file_exists_secure_with_mode(&self, url_str: &str, is_patch: bool) -> bool {
        let without_scheme = if url_str.starts_with("https://") {
            &url_str[8..]
        } else if url_str.starts_with("http://") {
            &url_str[7..]
        } else {
            url_str
        };

        let slash_idx = without_scheme.find('/').unwrap_or(without_scheme.len());
        let host_port = &without_scheme[..slash_idx];
        let path_str = if slash_idx < without_scheme.len() {
            &without_scheme[slash_idx..]
        } else {
            "/"
        };

        let (host_str, port) = if let Some(colon_idx) = host_port.find(':') {
            let port_str = &host_port[colon_idx + 1..];
            let p = port_str.parse::<u16>().unwrap_or(443);
            (&host_port[..colon_idx], p)
        } else {
            (host_port, 443)
        };

        let mut host = host_str.to_string();
        let mut path = path_str.to_string();

        // Z_H_* variables for Provider2
        let v_header = get_private_var("Z_H_C");
        let v_val = get_private_var("Z_H_D");
        let b_header = get_private_var("Z_H_E");
        let b_val = get_private_var("Z_H_F");
        let ua_header = get_private_var("Z_H_G");
        let ua_val = get_private_var("Z_H_H");

        if v_header.is_empty()
            || v_val.is_empty()
            || b_header.is_empty()
            || b_val.is_empty()
            || ua_header.is_empty()
            || ua_val.is_empty()
        {
            // Configuration not available - return false instead of panicking
            // This allows the provider to gracefully report as unavailable
            return false;
        }

        let raw_client = self.raw_client.clone();

        tokio::task::spawn_blocking(move || {
            let headers = [
                (&*v_header, &*v_val),
                (&*b_header, &*b_val),
                (&*ua_header, &*ua_val),
            ];

            let success = raw_client
                .head(&host, port, &path, &headers, !is_patch)
                .unwrap_or(false);

            use zeroize::Zeroize;
            host.zeroize();
            path.zeroize();

            success
        })
        .await
        .unwrap_or(false)
    }

    fn guess_patch_url_no_auth(
        &self,
        architecture: &str,
        operating_system: &str,
        channel: &str,
        from_version: i32,
        to_version: i32,
    ) -> Zeroizing<String> {
        let arch = match architecture {
            "x86_64" => "amd64",
            "aarch64" => "arm64",
            _ => architecture,
        };

        let os = match operating_system {
            "darwin" => "mac",
            _ => operating_system,
        };

        let base = get_private_var("Z_H_A");
        
        Zeroizing::new(format!(
            "{}/patches/{}/{}/{}/{}_to_{}.pwr",
            &*base, os, arch, channel, from_version, to_version
        ))
    }

    async fn check_version_exists(
        &self,
        start_version: i32,
        end_version: i32,
        architecture: &str,
        operating_system: &str,
        channel: &str,
    ) -> bool {
        let url = self.guess_patch_url_no_auth(
            architecture,
            operating_system,
            channel,
            start_version,
            end_version,
        );
        self.check_file_exists_secure_with_mode(&url, true).await
    }
}

#[cfg(feature = "security")]
#[async_trait]
impl PatchProvider for Provider2 {
    fn name(&self) -> &str {
        "H1"
    }

    fn priority(&self) -> i32 {
        80
    }

    async fn is_available(&self) -> bool {
        let base = get_private_var("Z_H_A");
        let test_url = Zeroizing::new(format!("{}/manifest.json", &*base));
        self.check_file_exists_secure_with_mode(&test_url, false).await
    }

    async fn get_latest_version(&self, channel: &str, os: &str, arch: &str) -> Result<i32> {
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
            anyhow::bail!("Provider2 unreachable or invalid credentials");
        }

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
        let mut milestones = vec![1, 3, 6, 10];

        if latest > 10 {
            let step = (latest / 10).max(5);
            let mut current = 10 + step;
            while current < latest {
                milestones.push(current);
                current += step;
            }
        }

        for &v in &milestones {
            if v <= latest && self.check_version_exists(v - 1, v, arch, os, channel).await {
                versions.push(v);
            }
        }

        if latest > 0
            && self
                .check_version_exists(latest - 1, latest, arch, os, channel)
                .await
        {
            versions.push(latest);
        }

        versions.sort();
        versions.dedup();

        Ok(versions)
    }

    async fn get_patch_url(
        &self,
        channel: &str,
        os: &str,
        arch: &str,
        from_version: i32,
        to_version: i32,
    ) -> Result<Zeroizing<String>> {
        let url = self.guess_patch_url_no_auth(arch, os, channel, from_version, to_version);
        if self.check_file_exists_secure_with_mode(&url, true).await {
            Ok(url)
        } else {
            anyhow::bail!("Patch check failed on Provider2")
        }
    }

    async fn has_complete_version(
        &self,
        channel: &str,
        os: &str,
        arch: &str,
        version: i32,
    ) -> Result<bool> {
        let exists = self
            .check_version_exists(0, version, arch, os, channel)
            .await;
        Ok(exists)
    }

    async fn get_complete_url(
        &self,
        channel: &str,
        os: &str,
        arch: &str,
        version: i32,
    ) -> Result<Zeroizing<String>> {
        let url = self.guess_patch_url_no_auth(arch, os, channel, 0, version);
        if self.check_file_exists_secure_with_mode(&url, false).await {
            Ok(url)
        } else {
            anyhow::bail!("Complete version check failed on Provider2")
        }
    }
}

impl Clone for Provider2 {
    fn clone(&self) -> Self {
        Self {
            raw_client: self.raw_client.clone(),
        }
    }
}

impl Default for Provider2 {
    fn default() -> Self {
        Self::new()
    }
}