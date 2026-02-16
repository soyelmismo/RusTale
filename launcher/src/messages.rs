/// Segregated message types for better organization
/// This separates UI events from Core logic events

use crate::config::{GameSettings, ProfilesConfig};
use crate::game::{LauncherStatus, mods::ModInfo, zip_mods::PatchManifest};
use crate::lang::Localization;
use std::path::PathBuf;

/// Core business logic events - data arriving from backend
#[derive(Debug, Clone)]
pub enum CoreMessage {
    /// Configuration has been loaded
    ConfigLoaded(ProfilesConfig, GameSettings, Localization),
    
    /// Status check completed
    StatusCheckCompleted {
        settings: GameSettings,
        status: LauncherStatus,
        latest_version: Option<i32>,
    },
    
    /// Download progress update
    DownloadProgress {
        progress: f32,
        sub_progress: f32,
        speed: String,
        total_bytes: u64,
        downloaded_bytes: u64,
        eta: Option<String>,
        current_step: Option<usize>,
    },
    
    /// Game launch result
    GameLaunched(Result<(), String>),
    
    /// Game has stopped
    GameStopped,
    
    /// Background image loaded
    BackgroundLoaded(Result<Vec<u8>, String>),
    
    /// Versions list received
    VersionsReceived(Vec<i32>),
    
    /// Installed versions received
    InstalledVersionsReceived(Vec<(i32, bool)>),
    
    /// Repair operation finished
    RepairFinished(Result<(), String>),
    
    /// Mods loaded (simple)
    ModsLoaded(Result<Vec<ModInfo>, String>),
    
    /// Mods loaded (complex with manifests)
    ModsLoadedComplex(Result<(Vec<ModInfo>, Vec<PatchManifest>), String>),
    
    /// Download error occurred
    DownloadError(String),
    
    /// Data migration started
    DataMoveStarted,
    
    /// Data migration finished
    DataMoveFinished(Result<PathBuf, String>),
    
    /// Migration progress update
    MigrationProgress(f32),
    
    /// Java info loaded
    JavaInfoLoaded,
    
    /// Server patch progress
    ServerPatchProgress(f32),
    
    /// Memory statistics update
    MemoryStatsUpdate,
    
    /// Watchdog check for stuck states
    WatchdogCheck,
}
