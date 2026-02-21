// Re-export common types from shared crate
pub use rustale_shared::config::{
    BaseThemeMode, GameSettings, OnlineFixMode, ThemeConfig,
};
pub use rustale_shared::profiles::{Profile, ProfilesConfig};

// Re-export system functions from engine crate
pub use rustale_engine::system::{
    LauncherConfig,
    InitializationConfig,
    default_lang, default_scale, default_true,
    get_bootstrap_path, get_app_dir, get_server_root_dir, get_identity_dir,
    save_bootstrap_path,
    load_profiles, save_profiles,
    load_settings, load_settings_sync, save_settings, save_settings_sync,
    load_initialization_config_sync,
    load_width_height,
};
