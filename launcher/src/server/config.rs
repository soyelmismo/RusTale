use serde::{Deserialize, Serialize};
use tokio::fs;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServerConfig {
    pub online_mode: String,  // "local" or "sanasol"
    pub branch: String,       // "release" or "pre-release"
    pub game_version: String, // "latest", "5", "12", etc.
    pub server_args: String,
    pub java_exec_args: String,
    pub tunnel_provider: Option<String>,
    pub use_direct_assets: bool, // Use assets directly from client without copying
    pub auth_domain: Option<String>, // Custom F2P auth domain (e.g., "auth.sanasol.ws")
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            online_mode: "local".to_string(),
            branch: "release".to_string(),
            game_version: "latest".to_string(),
            java_exec_args: "-Xms1G -Xmx4G -XX:+UseG1GC".to_string(),
            server_args: "--auth-mode insecure --assets Assets.zip".to_string(),
            tunnel_provider: Some("none".to_string()),
            use_direct_assets: true, // Enable direct asset access by default
            auth_domain: Some("127.0.0.000001".to_string()), // Local embedded server
        }
    }
}

impl ServerConfig {
    /// Fusiona automáticamente los campos faltantes con valores por defecto
    /// Funciona para cualquier campo nuevo agregado al struct sin modificar este código
    fn merge_missing_fields(&mut self) {
        let default = ServerConfig::default();
        
        // Convertir ambos structs a JSON Value para comparación
        let self_json = serde_json::to_value(&*self).unwrap_or_default();
        let default_json = serde_json::to_value(&default).unwrap_or_default();
        
        // Si self_json es un objeto, iterar sobre sus campos
        if let serde_json::Value::Object(mut self_obj) = self_json {
            if let serde_json::Value::Object(default_obj) = default_json {
                // Para cada campo en el default, verificar si falta o está vacío en self
                for (key, default_value) in default_obj {
                    match self_obj.get(&key) {
                        None => {
                            // Campo no existe en el archivo - usar default
                            self_obj.insert(key.clone(), default_value);
                        }
                        Some(serde_json::Value::Null) => {
                            // Campo es null - usar default
                            self_obj.insert(key, default_value);
                        }
                        Some(serde_json::Value::String(s)) if s.is_empty() => {
                            // Campo string vacío - usar default si no está vacío
                            if let serde_json::Value::String(default_str) = &default_value {
                                if !default_str.is_empty() {
                                    self_obj.insert(key, default_value);
                                }
                            }
                        }
                        _ => {
                            // Campo existe y tiene valor - mantenerlo
                        }
                    }
                }
            }
            
            // Convertir de vuelta a ServerConfig
            if let Ok(updated) = serde_json::from_value(serde_json::Value::Object(self_obj)) {
                *self = updated;
            }
        }
    }
}

pub async fn load_or_create(args: &crate::Args) -> ServerConfig {
    let path = crate::config::get_server_root_dir().join("server_config.toml");

    // 1. Cargar configuración existente (misma lógica que GameSettings)
    let mut config = if path.exists() {
        match fs::read_to_string(&path).await {
            Ok(content) => toml::from_str(&content).unwrap_or_default(),
            Err(_) => ServerConfig::default(),
        }
    } else {
        ServerConfig::default()
    };

    // 2. Fusionar automáticamente campos faltantes con valores por defecto
    // Esto funciona para CUALQUIER campo nuevo agregado al struct sin modificar este código
    config.merge_missing_fields();

    // 3. Sobreescribir con argumentos de CLI si existen (prioridad CLI)
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
    if let Some(t) = &args.tunnel {
        config.tunnel_provider = Some(t.clone());
    }

    // 4. Guardar cambios inmediatamente para "inicio rapido" futuro
    if let Ok(str) = toml::to_string_pretty(&config) {
        let _ = fs::write(&path, str).await;
    }

    config
}
