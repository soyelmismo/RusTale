pub mod auth;
pub mod crypto;

pub mod curseforge;
pub mod downloader;
pub mod install;
pub mod launch;
pub mod mods;
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
pub use launch::launch_game;
pub use paths::GamePaths;
pub use status::{LauncherStatus, calculate_status};
