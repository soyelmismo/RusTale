pub mod butler;
pub mod config;
pub mod errors;
pub mod java;
pub mod lang;
pub mod network;
pub mod oauth;
pub mod patch_api;
pub mod patcher;
pub mod paths;
pub mod profiles;
pub mod progress;

pub use progress::ProgressCallback;

// Re-exports
#[cfg(feature = "security")]
pub use rustale_security::init_shield;

/// Initialize the security subsystem.
/// This should be called once at application startup.
#[cfg(feature = "security")]
pub fn init_security() {
    // Initialize security shield once
    rustale_security::init_shield();
    // Pre-initialize the secure HTTP client
    let _ = &*network::SECURE_HTTP_CLIENT;
}

/// No-op when security is disabled
#[cfg(not(feature = "security"))]
pub fn init_security() {}

pub use config::*;
pub use lang::Localization;
pub use network::{HTTP_CLIENT, download_file};
pub use paths::*;
pub use profiles::*;
pub use progress::*;
pub use reqwest;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum LauncherStatus {
    Idle,
    CheckingUpdate,
    DownloadingUpdate { progress: f32, speed: String },
    InstallingUpdate,
    Launching,
    GameRunning,
    Error(String),
}
