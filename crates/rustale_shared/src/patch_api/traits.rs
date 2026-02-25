use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

#[async_trait]
pub trait PatchProvider: Send + Sync {
    fn name(&self) -> &str;
    fn priority(&self) -> i32;
    fn is_cloudflare(&self) -> bool { false }

    /// Retorna `true` si el proveedor está operativo.
    async fn is_available(&self) -> bool;

    /// Última versión disponible en el canal dado (números enteros únicamente).
    async fn get_latest_version(&self, channel: &str, os: &str, arch: &str) -> Result<i32>;

    /// Lista de versiones disponibles (para mostrar en la UI).
    async fn get_available_versions(&self, channel: &str, os: &str, arch: &str) -> Result<Vec<i32>>;

    /// Descarga el patch `from → to` directamente al disco.
    #[cfg(feature = "security")]
    async fn download_patch_secure(
        &self,
        channel: &str,
        os: &str,
        arch: &str,
        from_version: i32,
        to_version: i32,
        dest_path: &Path,
        cancel_token: Arc<AtomicBool>,
        progress_callback: Box<dyn Fn(f64, u64, u64) + Send + Sync>,
    ) -> Result<()>;
}
