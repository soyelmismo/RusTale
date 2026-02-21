pub mod agent;
pub mod aurora;
#[cfg(feature = "auth")]
pub mod auth;
#[cfg(feature = "patching")]
pub mod patch_api;


pub mod install;
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
