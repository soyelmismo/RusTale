pub mod agent;
pub mod aurora;
#[cfg(feature = "auth")]
pub mod auth;
#[cfg(feature = "patching")]
pub mod patch_api;

// Re-export install from rustale_shared
pub use rustale_shared::patch_api::{InstallPolicy, is_game_installed, get_installed_versions, get_local_version, save_local_version};

pub mod launch;
#[cfg(feature = "modding")]
pub mod mods;
#[cfg(feature = "modding")]
pub mod mods_api;
#[cfg(feature = "patching")]
pub mod patcher;
pub mod paths;
pub mod progress;

pub mod status;
#[cfg(feature = "modding")]
pub mod zip_mods;

// Re-exports for easier access
pub use paths::GamePaths;
pub use status::LauncherStatus;
