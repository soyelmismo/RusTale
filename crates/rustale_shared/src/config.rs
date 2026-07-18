use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;

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

pub async fn get_cache_dir(cache_type: &str) -> PathBuf {
    let cache_dir = get_app_dir().join("cache").join(cache_type);
    if !cache_dir.exists() {
        let _ = fs::create_dir_all(&cache_dir).await;
    }
    cache_dir
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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Default)]
pub enum OnlineFixMode {
    #[default]
    Local, // Emulador Local
    Sanasol, // Servidor externo
}

impl std::fmt::Display for OnlineFixMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local => write!(f, "Local Patch"),
            Self::Sanasol => write!(f, "Sanasol Patch"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BaseThemeMode {
    #[default]
    Black,
    Grey,
    Light,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ThemeConfig {
    pub accent_hex: String,
    pub base_mode: BaseThemeMode,
    pub saturation: f32, // 0.0 to 2.0 (1.0 is default)
    pub contrast: f32,   // 0.0 to 2.0 (1.0 is default)
    pub lsd_mode: bool,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            accent_hex: "#FFA845".to_string(), // RusTale Orange
            base_mode: BaseThemeMode::Grey,
            saturation: 1.0,
            contrast: 1.0,
            lsd_mode: false,
        }
    }
}

impl std::hash::Hash for ThemeConfig {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.accent_hex.hash(state);
        ((self.saturation * 1000.0) as i32).hash(state);
        ((self.contrast * 1000.0) as i32).hash(state);
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct GameSettings {
    #[serde(rename = "minMemory")]
    pub min_memory: u32,
    #[serde(rename = "maxMemory")]
    pub max_memory: u32,
    pub width: u32,
    pub height: u32,
    #[serde(default = "default_scale")]
    pub scale_factor: f32,
    #[serde(default)]
    pub fullscreen: bool,
    #[serde(rename = "javaArgs", default)]
    pub java_args: String,
    #[serde(rename = "gameDir", default)]
    pub game_dir: String,
    #[serde(default = "default_channel")]
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
    #[serde(default)]
    pub theme: ThemeConfig,
    #[serde(default)]
    pub safe_mode: bool,
    #[serde(default)]
    pub oauth_tokens: Option<crate::oauth::OAuthTokens>,
}

impl std::hash::Hash for GameSettings {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.min_memory.hash(state);
        self.max_memory.hash(state);
        self.width.hash(state);
        self.height.hash(state);
        ((self.scale_factor * 1000.0) as i32).hash(state);
        self.fullscreen.hash(state);
        self.java_args.hash(state);
        self.game_dir.hash(state);
        self.channel.hash(state);
        self.game_version.hash(state);
        self.language.hash(state);
        self.enable_news.hash(state);
        self.enable_online_fix.hash(state);
        self.online_fix_mode.hash(state);
        self.minimize_to_tray.hash(state);
        self.minimize_on_play.hash(state);
        self.quickplay.hash(state);
        self.enable_auto_update.hash(state);
        self.theme.hash(state);
        self.safe_mode.hash(state);
        // OAuth tokens don't affect visual/behavioral equality strictly for UI purposes,
        // so we don't hash them.
    }
}

fn default_channel() -> String {
    "release".to_string()
}

fn default_lang() -> String {
    "en-US".to_string()
}

fn default_scale() -> f32 {
    1.0
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
            scale_factor: 1.0,
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
            theme: ThemeConfig::default(),
            safe_mode: false,
        }
    }
}
