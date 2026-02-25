use crate::settings::SettingsMessage;
use crate::ui::mods_modal::ModsMessage;
use crate::ui::news_section::NewsMessage;
pub use crate::ui::server_panel::ServerMessage;
use iced::window;
use rustale_engine::core::updater::UpdaterMessage;
use rustale_engine::game::LauncherStatus;
use rustale_engine::game::mods::ModInfo;
use rustale_engine::game::zip_mods::PatchManifest;
use rustale_shared::config::GameSettings;
use rustale_shared::lang::Localization;
use rustale_shared::profiles::{Profile, ProfilesConfig};

#[derive(Debug, Clone)]
pub enum Message {
    // --- System Lifecycle ---
    Initialize,
    Tick(std::time::Instant),
    AppExit,
    CloseRequested,
    WindowResized(iced::Size),
    WindowResizedWithMaximized(iced::Size, bool),
    WindowEvent(window::Id, window::Event),
    ToggleWindowVisibility,

    // --- Core Signals ---
    CoreEvent(rustale_engine::core::signals::FromCore),
    CheckStatus,

    // --- Visuals & Input ---
    CursorMoved(iced::Point),
    MousePressed,
    MouseReleased,
    ShaderClicked,
    NextShader,
    NextShaderManual,
    BackgroundLoaded(Result<Vec<u8>, String>),
    MemoryStatsUpdate,

    // --- Config & State ---
    ConfigLoaded(ProfilesConfig, GameSettings, Localization),
    LanguageChangedInSettings(String),
    SaveSettings(GameSettings),

    // --- Game Actions ---
    StartGame,
    GameLaunched(Result<(), String>),
    GameStopped,
    DryRunFinished(GameSettings, LauncherStatus, Option<i32>),

    // --- Sub-Modules Wrappers ---
    Settings(SettingsMessage),
    Mods(ModsMessage),
    News(NewsMessage),
    LauncherUpdate(UpdaterMessage),

    // --- Quick Actions ---
    OpenSettings,
    CloseSettings,
    OpenMods,
    OpenFolder,

    // --- Version Management ---
    RequestVersionCheck(String),
    VersionsReceived(Vec<i32>),
    RequestDeleteVersion(u32),
    RequestRepairVersion(u32),
    RepairFinished(Result<(), String>),
    OpenVersionFolder(u32),
    InstalledVersionsReceived(Vec<(i32, bool)>),
    RequestInstalledVersions(String),

    // --- Profiles ---
    ProfileSelected(Profile),
    AddProfile,
    EditProfile(uuid::Uuid),
    DeleteProfile(uuid::Uuid),
    ProfileNameChanged(String),
    SaveProfileName,
    CancelProfileEdit,
    EditProfileUUID(uuid::Uuid),
    ProfileUUIDChanged(String),
    SaveProfileUUID,
    CancelProfileUUIDEdit,
    CopyUUID(String),
    GenerateRandomUUID,
    ToggleProfileDropdown,

    // --- Tray (Windows only) ---
    #[cfg(all(feature = "tray", windows))]
    TrayEvent(tray_icon::TrayIconEvent),
    #[cfg(all(feature = "tray", windows))]
    TrayMenuEvent(tray_icon::menu::MenuEvent),

    // --- Data Migration ---
    RequestMoveData(std::path::PathBuf),
    RequestUseDataLocation(std::path::PathBuf),
    DataMoveStarted,
    DataMoveFinished(Result<std::path::PathBuf, String>),
    MigrationProgress(f32),
    StartMigrationActual(std::path::PathBuf, std::path::PathBuf),

    // --- Java ---
    LoadJavaInfo,
    JavaInfoLoaded,

    // --- Mods Loading ---
    ModsLoaded(Result<Vec<ModInfo>, String>),
    ModsLoadedComplex(Result<(Vec<ModInfo>, Vec<PatchManifest>), String>),

    // --- Misc ---
    ToggleFullscreen,
    UpdateTotalSteps {
        total_steps: Option<usize>,
    },
    ProgressUpdate(rustale_engine::game::progress::ProgressPayload),
    DownloadError(String),
    ServerPatchProgress(f32),
    WatchdogCheck,
    RequestCacheStats,
    CancelAction,

    // ── Dedicated Server Panel ───────────────────────────────────────────────
    /// Open the server management panel (creates a ServerManager if not yet exists).
    OpenServerPanel,
    /// Close / hide the server management panel.
    CloseServerPanel,
    /// Messages routed to / from the server panel widget.
    Server(ServerMessage),

    // Fallback
    None,
}
