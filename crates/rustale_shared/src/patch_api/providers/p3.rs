#[cfg(feature = "security")]
use rustale_security::RawSecureClient;

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::patch_api::traits::PatchProvider;

#[cfg(feature = "security")]
pub struct Provider3 {
    raw_client: RawSecureClient,
}

#[cfg(feature = "security")]
impl Provider3 {
    pub fn new() -> Self {
        Self { raw_client: RawSecureClient::new() }
    }

    #[inline]
    async fn exec(
        &self,
        os: &str,
        arch: &str,
        channel: &str,
        from: i32,
        to: i32,
        accept_exception: bool,
        download: Option<(std::path::PathBuf, Arc<AtomicBool>, Box<dyn Fn(f64, u64, u64) + Send + Sync>)>,
    ) -> bool {
        let raw_client = self.raw_client.clone();
        let os      = os.to_string();
        let arch    = arch.to_string();
        let channel = channel.to_string();

        tokio::task::spawn_blocking(move || {
            const HDRS: &[(&str, &str)] = &[
                ("Z_V_C", "Z_V_D"),
            ];
            if let Some((dest, token, cb)) = download {
                raw_client.download_secure_file(
                    "Z_V_A", "Z_V_T", &os, &arch, &channel, from, to,
                    HDRS, dest.as_path(), token, cb,
                ).is_ok()
            } else {
                raw_client.request_head(
                    "Z_V_A", "Z_V_T", &os, &arch, &channel, from, to,
                    HDRS, accept_exception,
                ).unwrap_or(false)
            }
        })
        .await
        .unwrap_or(false)
    }

    async fn version_exists(&self, from: i32, to: i32, arch: &str, os: &str, ch: &str) -> bool {
        self.exec(os, arch, ch, from, to, false, None).await
    }

    async fn latest_version(&self, channel: &str, os: &str, arch: &str) -> Result<i32> {
        let (mut last, mut next, mut step) = (0, 1, 2);
        while next <= 100 {
            if self.version_exists(0, next, arch, os, channel).await {
                last = next; next += step; step += 1;
            } else { break; }
        }
        if last == 0 { anyhow::bail!("unreachable"); }
        let (mut lo, mut hi, mut res) = (last, next - 1, last);
        while lo <= hi {
            let mid = (lo + hi) / 2;
            if mid <= res { lo = mid + 1; continue; }
            if self.version_exists(0, mid, arch, os, channel).await { res = mid; lo = mid + 1; }
            else { hi = mid - 1; }
        }
        Ok(res)
    }
}

#[cfg(feature = "security")]
#[async_trait]
impl PatchProvider for Provider3 {
    fn name(&self) -> &str { "V" }
    fn priority(&self) -> i32 { 50 }
    fn is_cloudflare(&self) -> bool { false }

    async fn is_available(&self) -> bool {
        self.exec("linux", "amd64", "release", 0, 1, true, None).await
    }

    async fn get_latest_version(&self, channel: &str, os: &str, arch: &str) -> Result<i32> {
        self.latest_version(channel, os, arch).await
    }

    async fn get_available_versions(&self, channel: &str, os: &str, arch: &str) -> Result<Vec<i32>> {
        let latest = self.latest_version(channel, os, arch).await?;
        let mut milestones = vec![1, 3, 6, 10];
        if latest > 10 {
            let step = (latest / 10).max(5);
            let mut cur = 10 + step;
            while cur < latest { milestones.push(cur); cur += step; }
        }
        let mut versions = Vec::new();
        for &v in &milestones {
            if v <= latest && self.version_exists(v - 1, v, arch, os, channel).await {
                versions.push(v);
            }
        }
        if latest > 0 && self.version_exists(latest - 1, latest, arch, os, channel).await {
            versions.push(latest);
        }
        versions.sort(); versions.dedup();
        Ok(versions)
    }



    async fn download_patch_secure(
        &self, channel: &str, os: &str, arch: &str,
        from: i32, to: i32,
        dest_path: &std::path::Path,
        cancel_token: Arc<AtomicBool>,
        progress_callback: Box<dyn Fn(f64, u64, u64) + Send + Sync>,
    ) -> Result<()> {
        if self.exec(os, arch, channel, from, to, false, Some((dest_path.to_path_buf(), cancel_token, progress_callback))).await {
            Ok(())
        } else {
            anyhow::bail!("download failed")
        }
    }
}

impl Clone for Provider3 {
    fn clone(&self) -> Self { Self { raw_client: self.raw_client.clone() } }
}

impl Default for Provider3 {
    fn default() -> Self { Self::new() }
}