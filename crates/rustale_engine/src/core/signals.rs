use rustale_shared::config::GameSettings;
use rustale_shared::profiles::ProfilesConfig;
use crate::game::{
    LauncherStatus, mods::ModInfo, mods::ModInstallationRequest, mods_api::SearchResults,
    zip_mods::PatchManifest,
};
use crate::news::BlogPost;
use crate::core::errors::CoreError;
use std::path::PathBuf;

/// COMMANDS: Frontend -> Backend
/// "The user wants to do X"
#[derive(Debug, Clone)]
pub enum ToCore {
    // --- Lifecycle ---
    BootstrapSystem,
    StartLogicLoop,
    ExitApp,

    // --- State / IO Requests ---
    RequestInitialStatus(GameSettings),
    LoadJavaInfo,
    CheckForLauncherUpdates,
    PerformLauncherUpdate(String),
    RequestVersionCheck(String), // Kept for compatibility

    // --- Game Logic ---
    LaunchGame,
    StopGame,
    UpdateSettings(GameSettings),
    AbortOperation, // Generic cancel
    RequestRepairVersion(u32),
    RequestDeleteVersion(u32),

    // --- Data Management (NUEVO: Para eliminar IO de UI) ---
    InitializeProfiles(ProfilesConfig),
    SetCurrentProfile(uuid::Uuid),
    CreateProfile(String),
    UpdateProfileName(uuid::Uuid, String),
    UpdateProfileUuid(uuid::Uuid, uuid::Uuid),
    DeleteProfile(uuid::Uuid),
    SaveSettings(GameSettings),
    SaveProfile(ProfilesConfig),
    ImportProfile {
        path: PathBuf,
    },
    ImportProfilesFromMemory {
        profiles: Vec<rustale_shared::profiles::Profile>,
    },
    MigrateData {
        from: PathBuf,
        to: PathBuf,
    },

    // --- Modding (NUEVO: El core gestionará CurseForge/Zip) ---
    SearchMods {
        query: String,
        offset: u32,
        limit: u32,
    },
    LoadLocalMods {
        channel: String,
        version: String,
    }, // Replaces load_mods_task
    InstallMod(ModInstallationRequest),
    UpdateMod(ModInstallationRequest),
    UninstallMod(String),
    ToggleMod(String, bool),
    ToggleZipPatch(String, bool),
    CheckForUpdates, // Verificar actualizaciones de mods instalados
    LoadVersions(String), // Cargar versiones disponibles de un mod específico

    // --- News ---
    FetchNews,

    // --- Resource Management ---
    TrimMemory,
    OpenGameFolder,
    GetCacheStats,
}

/// EVENTS: Backend -> Frontend
/// "The system updated to state Y"
#[derive(Debug, Clone)]
pub enum FromCore {
    // Estado General
    BootstrapCompleted {
        settings: rustale_shared::config::GameSettings,
        profiles: rustale_shared::profiles::ProfilesConfig,
    },
    BootstrapFailed(String),
    StatusChanged(LauncherStatus),
    ProgressUpdate {
        phase: String,
        progress: f32,
        step_progress: f32,
        current_step: usize,
        total_steps: usize,
        msg_args: Vec<String>,
        stats: Option<String>,
    },
    Error {
        message: String,
        fatal: bool,
    },

    // Datos Específicos
    JavaInfoLoaded(crate::java::JavaInfo),
    ModsSearchLoaded(Result<SearchResults, CoreError>),
    LocalModsLoaded(Result<(Vec<ModInfo>, Vec<PatchManifest>), CoreError>),
    NewsLoaded(Result<Vec<BlogPost>, CoreError>),
    UpdatesLoaded(Result<(Vec<String>, std::collections::HashMap<String, Vec<crate::game::mods_api::GenericFile>>), CoreError>),
    VersionsLoaded(Result<(String, Vec<crate::game::mods_api::GenericFile>), CoreError>),
    // Note: Las imágenes se cargan en UI directamente o el core devuelve paths locales.
    // Por simplicidad, imágenes en UI vía async load está bien si es solo lectura,
    // pero idealmente Core descarga a cache y UI carga desde disco.

    // Confirmaciones
    SettingsSaved,
    ProfilesUpdated(rustale_shared::profiles::ProfilesConfig),
    ModOperationFinished(Result<(), CoreError>),
    GameStarted,
    GameStopped,
    ReadyToDisplay,

    // Launcher Update Results
    LauncherUpdateCheckResult(Result<Option<crate::core::updater::ReleaseInfo>, CoreError>),
    LauncherUpdateProgress(f32, String),
    LauncherUpdateFinished,
    MigrationFinished(Result<PathBuf, CoreError>),
    RepairOperationFinished(Result<(), CoreError>),

    // Update Events - now connected to UI
    UpdateAvailable(Option<crate::core::updater::ReleaseInfo>),
    UpdateDownloadProgress(f32),
    UpdateInstalled,
    UpdateError(String),

    // Legacy / Version Cache
    VersionCacheUpdated(Vec<i32>),
    InstalledVersionsLoaded(Vec<(i32, bool)>), // (version, is_latest_folder)

    // Mod Events
    ModSearchCompleted(Result<crate::game::mods_api::SearchResults, CoreError>),
    ModInstallProgress(String, f32),
    ModInstallCompleted(Result<String, CoreError>),
    ModUninstallCompleted(Result<String, CoreError>),

    // Cache Management
    CacheStatsLoaded(Result<crate::game::patch_api::CacheStats, CoreError>),

    // Fallback for generic failures if needed, or covered by Error struct
    OperationFailed {
        error: CoreError,
    },
}
