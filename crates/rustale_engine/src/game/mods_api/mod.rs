use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ModProvider {
    CurseForge,
    Modrinth, // Preparado para el futuro
    Local,    // Para mods instalados localmente sin repositorio
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

impl GenericMod {
    pub fn get_author_display(&self) -> &str {
        if self.author.is_empty() {
            "Unknown"
        } else {
            &self.author
        }
    }
    
    pub fn get_downloads_formatted(&self) -> String {
        if self.downloads >= 1_000_000 {
            format!("{:.1}M", self.downloads as f64 / 1_000_000.0)
        } else if self.downloads >= 1_000 {
            format!("{:.1}K", self.downloads as f64 / 1_000.0)
        } else {
            self.downloads.to_string()
        }
    }
}

// Representa una version especifica descargable
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericFile {
    pub file_id: String,
    pub name: String,         // Nombre del archivo (ej: "mod-v1.2.jar")
    pub version_name: String, // Nombre de la version (ej: "v1.2 Release")
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

#[cfg(test)]
mod tests {
    use super::*;

    // === GenericMod Tests ===

    #[test]
    fn test_generic_mod_author_display() {
        let mod_with_author = GenericMod {
            id: "123".to_string(),
            name: "Test Mod".to_string(),
            summary: "A test mod".to_string(),
            author: "TestAuthor".to_string(),
            logo_url: None,
            downloads: 1000,
            website_url: "https://example.com".to_string(),
            provider: ModProvider::CurseForge,
            latest_files: vec![],
        };
        
        assert_eq!(mod_with_author.get_author_display(), "TestAuthor");
        
        let mod_without_author = GenericMod {
            id: "456".to_string(),
            name: "Another Mod".to_string(),
            summary: "Another test".to_string(),
            author: "".to_string(),
            logo_url: None,
            downloads: 500,
            website_url: "https://example.com".to_string(),
            provider: ModProvider::CurseForge,
            latest_files: vec![],
        };
        
        assert_eq!(mod_without_author.get_author_display(), "Unknown");
    }

    #[test]
    fn test_generic_mod_downloads_formatting() {
        let million_mod = GenericMod {
            id: "1".to_string(),
            name: "Popular Mod".to_string(),
            summary: "".to_string(),
            author: "".to_string(),
            logo_url: None,
            downloads: 1_500_000,
            website_url: "".to_string(),
            provider: ModProvider::CurseForge,
            latest_files: vec![],
        };
        assert_eq!(million_mod.get_downloads_formatted(), "1.5M");
        
        let thousand_mod = GenericMod {
            id: "2".to_string(),
            name: "Medium Mod".to_string(),
            summary: "".to_string(),
            author: "".to_string(),
            logo_url: None,
            downloads: 25_500,
            website_url: "".to_string(),
            provider: ModProvider::CurseForge,
            latest_files: vec![],
        };
        assert_eq!(thousand_mod.get_downloads_formatted(), "25.5K");
        
        let small_mod = GenericMod {
            id: "3".to_string(),
            name: "Small Mod".to_string(),
            summary: "".to_string(),
            author: "".to_string(),
            logo_url: None,
            downloads: 150,
            website_url: "".to_string(),
            provider: ModProvider::CurseForge,
            latest_files: vec![],
        };
        assert_eq!(small_mod.get_downloads_formatted(), "150");
    }

    // === GenericFile Display Tests ===

    #[test]
    fn test_generic_file_display_with_version_name() {
        let file = GenericFile {
            file_id: "file-123".to_string(),
            name: "mod-v1.2.0.jar".to_string(),
            version_name: "v1.2.0 Release".to_string(),
            download_url: Some("https://example.com/mod.jar".to_string()),
            release_date: chrono::Utc::now(),
            game_versions: vec!["1.20.1".to_string()],
        };
        
        let display = format!("{}", file);
        assert!(display.contains("v1.2.0 Release"));
        assert!(display.contains("1.20.1"));
    }

    #[test]
    fn test_generic_file_display_without_version_name() {
        let file = GenericFile {
            file_id: "file-456".to_string(),
            name: "my-awesome-mod.jar".to_string(),
            version_name: "".to_string(),
            download_url: Some("https://example.com/mod.jar".to_string()),
            release_date: chrono::Utc::now(),
            game_versions: vec![],
        };
        
        let display = format!("{}", file);
        assert!(display.contains("my-awesome-mod"));
        assert!(display.contains("Any"));
    }

    #[test]
    fn test_generic_file_display_truncates_long_game_versions() {
        let file = GenericFile {
            file_id: "file-789".to_string(),
            name: "mod.jar".to_string(),
            version_name: "v1.0".to_string(),
            download_url: None,
            release_date: chrono::Utc::now(),
            game_versions: vec![
                "1.20.1".to_string(),
                "1.20.2".to_string(),
                "1.20.3".to_string(),
                "1.20.4".to_string(),
                "1.20.5".to_string(),
                "1.20.6".to_string(),
            ],
        };
        
        let display = format!("{}", file);
        // Should truncate because joined string is > 25 chars
        assert!(display.contains("..."));
    }

    // === ModProvider JSON Contract ===
    // Important: This is serialized to disk in manifest files

    #[test]
    fn test_mod_provider_json_contract() {
        // Verify all providers serialize/deserialize correctly
        for provider in [ModProvider::CurseForge, ModProvider::Modrinth, ModProvider::Local] {
            let json = serde_json::to_string(&provider).unwrap();
            let decoded: ModProvider = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, provider);
        }
    }

    // === SearchResults Tests ===

    #[test]
    fn test_search_results_empty() {
        let results = SearchResults {
            mods: vec![],
            total_count: 0,
        };
        
        assert!(results.mods.is_empty());
        assert_eq!(results.total_count, 0);
    }

    #[test]
    fn test_search_results_with_mods() {
        let results = SearchResults {
            mods: vec![
                GenericMod {
                    id: "1".to_string(),
                    name: "Mod A".to_string(),
                    summary: "First mod".to_string(),
                    author: "Author A".to_string(),
                    logo_url: None,
                    downloads: 1000,
                    website_url: "".to_string(),
                    provider: ModProvider::CurseForge,
                    latest_files: vec![],
                },
                GenericMod {
                    id: "2".to_string(),
                    name: "Mod B".to_string(),
                    summary: "Second mod".to_string(),
                    author: "Author B".to_string(),
                    logo_url: None,
                    downloads: 2000,
                    website_url: "".to_string(),
                    provider: ModProvider::Modrinth,
                    latest_files: vec![],
                },
            ],
            total_count: 2,
        };
        
        assert_eq!(results.mods.len(), 2);
        assert_eq!(results.total_count, 2);
    }
}

#[cfg(test)]
mod http_integration_tests {
    use super::*;
    use super::curseforge::CurseForgeRepository;
    use wiremock::{MockServer, Mock, ResponseTemplate};
    use wiremock::matchers::method;

    /// Helper to create a mock CurseForge search response
    fn mock_search_response() -> serde_json::Value {
        serde_json::json!({
            "data": [
                {
                    "id": 12345,
                    "name": "Test Mod",
                    "summary": "A test mod for testing",
                    "authors": [{"name": "TestAuthor"}],
                    "logo": {"thumbnailUrl": "https://example.com/logo.png"},
                    "downloadCount": 100000,
                    "links": {"websiteUrl": "https://curseforge.com/mods/test"},
                    "latestFiles": []
                }
            ],
            "pagination": {
                "totalCount": 1,
                "pageSize": 20,
                "index": 0
            }
        })
    }

    /// Helper to create a mock CurseForge versions response
    fn mock_versions_response() -> serde_json::Value {
        serde_json::json!([
            {
                "id": 111,
                "displayName": "v1.0.0",
                "fileName": "test-mod-1.0.0.jar",
                "downloadUrl": "https://example.com/files/test-mod-1.0.0.jar",
                "fileDate": "2024-01-01T00:00:00Z",
                "gameVersions": ["1.20.1", "1.20.2"]
            }
        ])
    }

    #[tokio::test]
    async fn test_search_success() {
        let mock_server = MockServer::start().await;
        
        // El repo añade /mods/search a api_base, así que mockeamos ese path
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_search_response()))
            .mount(&mock_server)
            .await;

        let client = rustale_shared::reqwest::Client::new();
        let repo = CurseForgeRepository::new_with_client_and_base(client, mock_server.uri());
        
        let result = repo.search("test", 0, 20).await;
        
        assert!(result.is_ok());
        let results = result.unwrap();
        assert_eq!(results.mods.len(), 1);
        assert_eq!(results.mods[0].name, "Test Mod");
        assert_eq!(results.mods[0].author, "TestAuthor");
    }

    #[tokio::test]
    async fn test_search_server_error_500() {
        let mock_server = MockServer::start().await;
        
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500).set_body_json(serde_json::json!({
                "error": "Internal Server Error"
            })))
            .mount(&mock_server)
            .await;

        let client = rustale_shared::reqwest::Client::new();
        let repo = CurseForgeRepository::new_with_client_and_base(client, mock_server.uri());
        
        let result = repo.search("test", 0, 20).await;
        
        // Should return error, not panic
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("500") || err.to_string().contains("error"));
    }

    #[tokio::test]
    async fn test_search_rate_limited_429() {
        let mock_server = MockServer::start().await;
        
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(429)
                .set_body_json(serde_json::json!({
                    "error": "Rate limit exceeded",
                    "retryAfter": 60
                })))
            .mount(&mock_server)
            .await;

        let client = rustale_shared::reqwest::Client::new();
        let repo = CurseForgeRepository::new_with_client_and_base(client, mock_server.uri());
        
        let result = repo.search("test", 0, 20).await;
        
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_search_malformed_json() {
        let mock_server = MockServer::start().await;
        
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200)
                .set_body_string("{ this is not valid json }"))
            .mount(&mock_server)
            .await;

        let client = rustale_shared::reqwest::Client::new();
        let repo = CurseForgeRepository::new_with_client_and_base(client, mock_server.uri());
        
        let result = repo.search("test", 0, 20).await;
        
        // Should gracefully handle malformed JSON
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_search_empty_results() {
        let mock_server = MockServer::start().await;
        
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [],
                "pagination": {
                    "totalCount": 0,
                    "pageSize": 20,
                    "index": 0
                }
            })))
            .mount(&mock_server)
            .await;

        let client = rustale_shared::reqwest::Client::new();
        let repo = CurseForgeRepository::new_with_client_and_base(client, mock_server.uri());
        
        let result = repo.search("nonexistent-mod-xyz123", 0, 20).await;
        
        assert!(result.is_ok());
        let results = result.unwrap();
        assert!(results.mods.is_empty());
        assert_eq!(results.total_count, 0);
    }

    #[tokio::test]
    async fn test_get_versions_success() {
        let mock_server = MockServer::start().await;
        
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": mock_versions_response().as_array().unwrap().clone()
            })))
            .mount(&mock_server)
            .await;

        let client = rustale_shared::reqwest::Client::new();
        let repo = CurseForgeRepository::new_with_client_and_base(client, mock_server.uri());
        
        let result = repo.get_versions("12345").await;
        
        assert!(result.is_ok());
        let versions = result.unwrap();
        assert_eq!(versions.len(), 1);
        // version_name es igual a file_name en la conversión (CF no siempre tiene version name separado)
        assert_eq!(versions[0].name, "test-mod-1.0.0.jar");
        assert_eq!(versions[0].version_name, "test-mod-1.0.0.jar");
        assert!(versions[0].download_url.is_some());
    }

    #[tokio::test]
    async fn test_get_versions_mod_not_found_404() {
        let mock_server = MockServer::start().await;
        
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "error": "Mod not found"
            })))
            .mount(&mock_server)
            .await;

        let client = rustale_shared::reqwest::Client::new();
        let repo = CurseForgeRepository::new_with_client_and_base(client, mock_server.uri());
        
        let result = repo.get_versions("99999").await;
        
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_versions_unauthorized_401() {
        let mock_server = MockServer::start().await;
        
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error": "Invalid API key"
            })))
            .mount(&mock_server)
            .await;

        let client = rustale_shared::reqwest::Client::new();
        let repo = CurseForgeRepository::new_with_client_and_base(client, mock_server.uri());
        
        let result = repo.get_versions("123").await;
        
        assert!(result.is_err());
    }
}
