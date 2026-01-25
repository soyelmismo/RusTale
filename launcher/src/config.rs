use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;

// --- DATA STRUCTURES ---

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct Profile {
    pub id: String,
    pub name: String,
}

impl std::fmt::Display for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

// File: profiles.toml
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct ProfilesConfig {
    pub profiles: Vec<Profile>,
    #[serde(rename = "current_profile")]
    pub current_profile: String,
}

impl Default for ProfilesConfig {
    fn default() -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        Self {
            profiles: vec![Profile {
                id: id.clone(),
                name: "Player".to_string(),
            }],
            current_profile: id,
        }
    }
}

impl ProfilesConfig {
    pub fn get_active_profile(&self) -> Option<Profile> {
        self.profiles
            .iter()
            .find(|p| p.id == self.current_profile)
            .cloned()
    }

    pub fn get_current_profile_name(&self) -> String {
        self.get_active_profile()
            .map(|p| p.name)
            .unwrap_or_else(|| "Player".to_string())
    }

    pub fn add_profile(&mut self, name: String) {
        let new_profile = Profile {
            id: uuid::Uuid::new_v4().to_string(),
            name,
        };
        self.current_profile = new_profile.id.clone();
        self.profiles.push(new_profile);
    }

    pub fn update_profile(&mut self, id: &str, new_name: String) {
        if let Some(profile) = self.profiles.iter_mut().find(|p| p.id == id) {
            profile.name = new_name;
        }
    }

    pub fn update_profile_uuid(&mut self, old_id: &str, new_uuid: String) {
        // Verificamos que el ID exista
        if let Some(pos) = self.profiles.iter().position(|p| p.id == old_id) {
            // Actualizamos el ID en el vector
            self.profiles[pos].id = new_uuid.clone();

            // Si el perfil editado era el seleccionado actualmente, actualizamos la referencia
            if self.current_profile == old_id {
                self.current_profile = new_uuid;
            }
        }
    }

    pub fn delete_profile(&mut self, id: &str) {
        if self.profiles.len() <= 1 {
            return;
        }

        self.profiles.retain(|p| p.id != id);

        if self.current_profile == id {
            if let Some(first) = self.profiles.first() {
                self.current_profile = first.id.clone();
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Default)]
pub enum OnlineFixMode {
    #[default]
    Local, // Emulador Local (tu código server.rs)
    Sanasol, // Servidor externo
}

impl std::fmt::Display for OnlineFixMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local => write!(f, "Local Patch"),
            Self::Sanasol => write!(f, "Sanasol Patch (broken)"),
        }
    }
}

// File: settings.toml
// Added 'Hash' and 'Eq' to allow usage in Iced Subscriptions
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
#[serde(default)]
pub struct GameSettings {
    #[serde(rename = "minMemory")]
    pub min_memory: u32,
    #[serde(rename = "maxMemory")]
    pub max_memory: u32,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub fullscreen: bool,
    #[serde(rename = "javaArgs", default)]
    pub java_args: String,
    #[serde(rename = "gameDir", default)]
    pub game_dir: String,
    #[serde(default)]
    pub channel: String,
    #[serde(rename = "gameVersion", default)]
    pub game_version: u32,
    #[serde(default = "default_lang")]
    pub language: String,
    #[serde(default = "default_true")]
    pub enable_news: bool,
    #[serde(default)]
    pub enable_online_fix: bool,
    #[serde(default)]
    pub online_fix_mode: OnlineFixMode,
    #[serde(default)]
    pub minimize_to_tray: bool,
    #[serde(default)]
    pub minimize_on_play: bool,
    #[serde(default)]
    pub quickplay: bool,
    #[serde(default = "default_true")]
    pub enable_auto_update: bool,
}

fn default_lang() -> String {
    "en-US".to_string()
}

fn default_true() -> bool {
    true
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            min_memory: 2,
            max_memory: 4,
            width: 1024,
            height: 640,
            fullscreen: false,
            java_args: "-XX:+UseG1GC -Dsun.rmi.dgc.server.gcInterval=2147483646 -XX:+UnlockExperimentalVMOptions -XX:G1NewSizePercent=20 -XX:G1ReservePercent=20 -XX:MaxGCPauseMillis=50 -XX:G1HeapRegionSize=32M".to_string(),
            game_dir: "".to_string(),
            channel: "release".to_string(),
            game_version: 0,
            language: "en-US".to_string(),
            enable_news: true,
            enable_online_fix: true,
            online_fix_mode: OnlineFixMode::Local,
            minimize_to_tray: false,
            minimize_on_play: false,
            quickplay: false,
            enable_auto_update: true,
        }
    }
}

// --- DATA DIR LOGIC (BOOTSTRAP) ---

#[derive(Debug, Serialize, Deserialize)]
pub struct LauncherConfig {
    pub data_dir: Option<PathBuf>,
}

pub fn get_bootstrap_path() -> PathBuf {
    // 1. Search launcher.toml next to the executable (Portable / Server Mode)
    if let Ok(mut exe_path) = std::env::current_exe() {
        exe_path.pop(); // Remove the exe name
        let local_config = exe_path.join("launcher.toml");
        if local_config.exists() {
            return local_config;
        }
    }

    // 2. DEFAULT
    if let Some(base_dirs) = directories::BaseDirs::new() {
        let config_dir = base_dirs.config_dir().join("RusTale");
        if !config_dir.exists() {
            let _ = std::fs::create_dir_all(&config_dir);
        }
        return config_dir.join("launcher.toml");
    }

    // Fallback
    PathBuf::from("launcher.toml")
}

pub fn get_app_dir() -> PathBuf {
    // 0. MAXIMUM PRIORITY: Environment variable (Useful for server scripts or docker)
    if let Ok(env_dir) = std::env::var("RUSTALE_DATA_DIR") {
        return PathBuf::from(env_dir);
    }

    let bootstrap_path = get_bootstrap_path();

    // 1. If launcher.toml exists (either local or in AppData), we read it
    if bootstrap_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&bootstrap_path) {
            if let Ok(cfg) = toml::from_str::<LauncherConfig>(&content) {
                if let Some(dir) = cfg.data_dir {
                    return dir;
                }
            }
        }
    }

    // 2. If no configuration exists, we decide the default:
    // If we are in the user's folder (appdata), we use that.
    // BUT, if we want default portable behavior for the server,
    // we could check if "server_config.toml" exists locally.

    if let Ok(mut exe_path) = std::env::current_exe() {
        exe_path.pop();
        // If we detect a local server config, we stay here
        if exe_path.join("server_config.toml").exists()
            || exe_path.join("start_server.bat").exists()
        {
            return exe_path;
        }
    }

    // 3. Fallback to standard system (Client normal)
    if let Some(base_dirs) = directories::BaseDirs::new() {
        base_dirs.config_dir().join("RusTale")
    } else if let Ok(mut exe_path) = std::env::current_exe() {
        exe_path.pop();
        exe_path.join("RusTale_Data")
    } else {
        PathBuf::from("C:\\RusTale")
    }
}

pub fn get_server_root_dir() -> PathBuf {
    get_app_dir().join("server")
}

pub fn get_identity_dir() -> PathBuf {
    get_server_root_dir().join("identity")
}

pub fn save_bootstrap_path(new_data_dir: &PathBuf) -> anyhow::Result<()> {
    let cfg = LauncherConfig {
        data_dir: Some(new_data_dir.clone()),
    };

    let bootstrap_path = get_bootstrap_path();

    if let Some(parent) = bootstrap_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let toml_str = toml::to_string_pretty(&cfg)?;
    std::fs::write(bootstrap_path, toml_str)?;
    Ok(())
}

// --- LOAD/SAVE LOGIC (ACTUALIZADA) ---

// CAMBIO 3: Lógica de actualización automática de esquema para Profiles
pub async fn load_profiles() -> ProfilesConfig {
    let path = get_path("profiles.toml");
    let config = match fs::read_to_string(&path).await {
        Ok(content) => {
            // Intentamos parsear. Gracias a #[serde(default)], los campos faltantes se rellenan.
            // Si el parseo falla catastróficamente (sintaxis inválida), usamos default.
            toml::from_str(&content).unwrap_or_default()
        }
        Err(_) => ProfilesConfig::default(),
    };

    // ALGORITMO DE LIMPIEZA: Guardamos inmediatamente.
    // 1. Si había claves obsoletas en el archivo, se pierden al leer en el struct y no se escriben de nuevo.
    // 2. Si faltaban claves nuevas, el struct las tiene por default y se escriben ahora.
    // 3. Los valores existentes válidos se mantienen.
    let _ = save_profiles(&config).await;

    config
}

pub async fn save_profiles(cfg: &ProfilesConfig) -> anyhow::Result<()> {
    save_file(cfg, "profiles.toml").await
}

// CAMBIO 4: Lógica de actualización automática de esquema para Settings
pub async fn load_settings() -> GameSettings {
    load_settings_sync()
}

pub fn load_settings_sync() -> GameSettings {
    let path = get_path("settings.toml");
    let config = match std::fs::read_to_string(&path) {
        Ok(content) => toml::from_str(&content).unwrap_or_default(),
        Err(_) => GameSettings::default(),
    };

    // Validación extra (opcional): Asegurar que los valores leídos tienen sentido
    let mut safe_config = config;
    if safe_config.width < 100 {
        safe_config.width = 480;
    }
    if safe_config.height < 100 {
        safe_config.height = 390;
    }

    // Guardamos la versión "saneada" y actualizada del esquema.
    let _ = save_settings_sync(&safe_config);

    safe_config
}

pub async fn save_settings(cfg: &GameSettings) -> anyhow::Result<()> {
    save_settings_sync(cfg)
}

pub fn save_settings_sync(cfg: &GameSettings) -> anyhow::Result<()> {
    let toml_str = toml::to_string_pretty(cfg)?;
    let path = get_path("settings.toml");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(path, toml_str)?;
    Ok(())
}

pub fn load_initialization_config_sync() -> InitializationConfig {
    let settings = load_settings_sync();
    InitializationConfig {
        quickplay: settings.quickplay,
    }
}
pub struct InitializationConfig {
    pub quickplay: bool,
}

pub fn load_width_height() -> (f32, f32) {
    let settings = load_settings_sync();
    (settings.width as f32, settings.height as f32)
}

async fn save_file<T: Serialize>(data: &T, filename: &str) -> anyhow::Result<()> {
    let path = get_path(filename);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let toml_str = toml::to_string_pretty(data)?;
    fs::write(path, toml_str).await?;
    Ok(())
}

fn get_path(filename: &str) -> PathBuf {
    get_app_dir().join(filename)
}

pub async fn get_cache_dir(cache_type: &str) -> PathBuf {
    let cache_dir = get_app_dir().join("cache").join(cache_type);
    if !cache_dir.exists() {
        let _ = fs::create_dir_all(&cache_dir).await;
    }
    cache_dir
}
