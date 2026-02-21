pub mod butler;
pub mod config;
pub mod errors;
pub mod java;
pub mod lang;
pub mod network;
pub mod patch_api;
pub mod paths;
pub mod profiles;
pub mod progress;

pub use progress::ProgressCallback;

// Re-exports
pub use rustale_security::init_shield;

pub use config::*;
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
