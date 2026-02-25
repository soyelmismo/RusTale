use anyhow::Result;
use std::path::PathBuf;

use crate::patch_api::mod_manager::PatchApiManager;
use crate::patch_api::utils::get_arch_name;
use crate::patch_api::types::{GameVersionInfo, get_local_version};

/// Manager para operaciones de versión del juego
#[derive(Clone)]
pub struct VersionManager {}

impl VersionManager {
    pub fn new() -> Self {
        Self {}
    }

    /// Obtiene información completa de versión para un canal
    pub async fn get_version_info(
        &self,
        base_dir: &PathBuf,
        channel: &str,
        user_version: i32,
    ) -> Result<GameVersionInfo> {
        let local_version = get_local_version(base_dir, channel)
            .await
            .unwrap_or(0);

        // Obtener última versión mediante el manager de la API (vía security)
        let latest = PatchApiManager::get_latest_version_static(
            channel,
            std::env::consts::OS,
            get_arch_name(),
        )
        .await?;

        // Generar lista por defecto
        let mut available: Vec<i32> = (1..=latest).collect();
        available.reverse();

        // Intentar obtener versiones disponibles reales (si el provider lo soporta)
        let available_versions_from_fallback = match PatchApiManager::get_available_versions_static(
            channel,
            std::env::consts::OS,
            get_arch_name(),
        )
        .await
        {
            Ok(versions) => Some(versions),
            Err(_) => None,
        };

        Ok(GameVersionInfo {
            user_version,
            current_local: local_version,
            latest_remote: latest,
            available_versions: available,
            available_versions_from_fallback,
            update_available: user_version == 0 && local_version < latest,
        })
    }

    /// Encuentra la última versión disponible
    pub async fn find_latest_version(&self, channel: &str, _start_hint: Option<i32>) -> Result<i32> {
        let os = std::env::consts::OS;
        let arch = get_arch_name();

        // El hint ya no se usa para comprobaciones manuales; se delega todo al manager/security
        PatchApiManager::get_latest_version_static(channel, os, arch).await
    }
}
