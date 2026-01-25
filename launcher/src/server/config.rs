use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServerConfig {
    pub online_mode: String,  // "local" or "sanasol"
    pub branch: String,       // "release" or "pre-release"
    pub game_version: String, // "latest", "5", "12", etc.
    pub server_args: String,
    pub java_exec_args: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            online_mode: "local".to_string(),
            branch: "release".to_string(),
            game_version: "latest".to_string(),
            java_exec_args: "-Xms1G -Xmx4G -XX:+UseG1GC".to_string(),
            server_args: "--auth-mode insecure --assets Assets.zip".to_string(),
        }
    }
}

pub async fn load_or_create(args: &crate::Args) -> ServerConfig {
    let path = PathBuf::from("server_config.toml");

    // 1. Cargar existente
    let mut config = if path.exists() {
        match fs::read_to_string(&path).await {
            Ok(content) => toml::from_str(&content).unwrap_or_default(),
            Err(_) => ServerConfig::default(),
        }
    } else {
        ServerConfig::default()
    };

    // 2. Sobreescribir con argumentos de CLI si existen (prioridad CLI)
    if let Some(m) = &args.online_mode {
        config.online_mode = m.clone();
    }
    if let Some(b) = &args.branch {
        config.branch = b.clone();
    }
    if let Some(v) = &args.game_version {
        config.game_version = v.clone();
    }
    if let Some(a) = &args.java_exec_args {
        config.java_exec_args = a.clone();
    }
    if let Some(a) = &args.server_args {
        config.server_args = a.clone();
    }

    // 3. Guardar cambios inmediatamente para "inicio rápido" futuro
    if let Ok(str) = toml::to_string_pretty(&config) {
        let _ = fs::write(&path, str).await;
    }

    config
}
