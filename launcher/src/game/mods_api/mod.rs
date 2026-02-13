use async_trait::async_trait;
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ModProvider {
    CurseForge,
    Modrinth, // Preparado para el futuro
}

// Representa un Mod en el navegador (resultados de busqueda)
#[derive(Debug, Clone)]
pub struct GenericMod {
    pub id: String, // En CF es int, en Modrinth string. Usaremos String uniformemente.
    pub name: String,
    pub summary: String,
    pub author: String,
    pub logo_url: Option<String>,
    pub downloads: u64,
    pub website_url: String,
    pub provider: ModProvider,
    // Metadatos internos del proveedor (guardar el struct original si hace falta)
    pub latest_files: Vec<GenericFile>, 
}

// Representa una version especifica descargable
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericFile {
    pub file_id: String,
    pub name: String,        // Nombre del archivo (ej: "mod-v1.2.jar")
    pub version_name: String,// Nombre de la version (ej: "v1.2 Release")
    pub download_url: Option<String>,
    pub release_date: chrono::DateTime<chrono::Utc>,
    pub game_versions: Vec<String>, // Versiones compatibles del juego
}

// Esto define que texto muestra el Dropdown
impl std::fmt::Display for GenericFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Usar version_name si esta disponible y es diferente del nombre del archivo
        // sino extraer version del mod del nombre del archivo
        let display_version = if !self.version_name.is_empty() && self.version_name != self.name {
            &self.version_name
        } else if let Some(name_without_ext) = self.name.strip_suffix(".jar") {
            name_without_ext
        } else {
            &self.name
        };
        
        // Mostrar versiones de juego compatibles de forma truncada si es muy larga
        let game_versions = if self.game_versions.is_empty() {
            "Any".to_string()
        } else {
            let joined = self.game_versions.join(", ");
            if joined.len() > 25 {
                format!("{}...", &joined[..25])
            } else {
                joined
            }
        };
        
        // Formato: "version_name [game_versions]" - mas claro para el usuario
        write!(f, "{} [{}]", display_version, game_versions)
    }
}

// Contrato que deben cumplir CurseForge y Modrinth
#[async_trait]
pub trait ModRepository: Send + Sync {
    async fn search(&self, query: &str, index: u32, page_size: u32) -> Result<SearchResults>;
    
    // Obtener versiones disponibles para un Mod
    async fn get_versions(&self, mod_id: &str) -> Result<Vec<GenericFile>>;
}

#[derive(Debug, Clone)]
pub struct SearchResults {
    pub mods: Vec<GenericMod>,
    pub total_count: u32,
}

pub mod curseforge;
