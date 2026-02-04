pub mod agent;
pub mod auth;
pub mod crypto;

pub mod downloader;
pub mod install;
pub mod launch;
pub mod mods;
pub mod mods_api;
pub mod patcher;
pub mod paths;
pub mod progress;
pub mod repair;
pub mod runner;
pub mod server;
pub mod status;
pub mod zip_mods;

// Re-exports for easier access
pub use install::ensure_installed;
pub use launch::{launch_game, launch_game_with_async_agent};
pub use paths::GamePaths;
pub use agent::quick_verify_agent;
pub use status::{LauncherStatus, calculate_status};
