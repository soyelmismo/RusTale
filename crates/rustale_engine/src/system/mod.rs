use std::path::PathBuf;
use serde::Serialize;
use tokio::fs;
use rustale_shared::config::GameSettings;
use rustale_shared::profiles::ProfilesConfig;
pub mod lifecycle;

// Re-export specific items for convenience
pub use rustale_shared::config::{
    BaseThemeMode, OnlineFixMode, ThemeConfig, LauncherConfig,
    get_bootstrap_path, get_server_root_dir, get_identity_dir, save_bootstrap_path,
    get_app_dir
};



// --- LOAD/SAVE LOGIC ---

pub async fn load_profiles() -> ProfilesConfig {
    let path = get_path("profiles.toml");
    let config = match fs::read_to_string(&path).await {
        Ok(content) => {
            toml::from_str(&content).unwrap_or_default()
        }
        Err(_) => ProfilesConfig::default(),
    };
    config
}

pub async fn save_profiles(cfg: &ProfilesConfig) -> anyhow::Result<()> {
    save_file(cfg, "profiles.toml").await
}

pub async fn load_settings() -> GameSettings {
    load_settings_sync()
}

pub fn load_settings_sync() -> GameSettings {
    let path = get_path("settings.toml");
    let config = match std::fs::read_to_string(&path) {
        Ok(content) => toml::from_str(&content).unwrap_or_default(),
        Err(_) => GameSettings::default(),
    };

    let mut safe_config = config;
    if safe_config.width < 100 {
        safe_config.width = 480;
    }
    if safe_config.height < 100 {
        safe_config.height = 390;
    }

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

pub struct InitializationConfig {
    pub quickplay: bool,
}

pub fn load_initialization_config_sync() -> InitializationConfig {
    let settings = load_settings_sync();
    InitializationConfig {
        quickplay: settings.quickplay,
    }
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

// Function helpers
pub fn default_lang() -> String {
    "en-US".to_string()
}
pub fn default_scale() -> f32 {
    1.0
}
pub fn default_true() -> bool {
    true
}
