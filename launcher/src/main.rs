#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use clap::Parser;
use futures::SinkExt;
use iced::widget::{Space, column, container, image, row, stack};
use iced::{Color, ContentFit, Element, Length, Size, Subscription, Task, Theme, window};
use tray_icon::{
    TrayIconBuilder,
    menu::{Menu, MenuItem},
};

mod app;
mod config;
mod game;
mod java;
mod lang;
mod news;
mod server;
mod settings;
mod theme;
mod ui;
mod updater;
mod util;

use crate::config::{GameSettings, Profile, ProfilesConfig};
use crate::game::install::InstallPolicy;
use crate::game::mods::ModInfo;
use crate::game::zip_mods::PatchManifest;
use crate::game::{GamePaths, LauncherStatus};
use crate::lang::Localization;
use crate::settings::{SettingsMessage, SettingsState};
use crate::ui::mods_modal::{ModsMessage, ModsState};
use crate::ui::news_section::{NewsMessage, NewsSection};
use crate::ui::{control_section, profile_card}; // Import the struct

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Start in Quickplay mode (no UI)
    #[arg(long)]
    quickplay: bool,

    // --- SERVER ARGUMENTS ---
    /// Enable Dedicated Server Mode (CLI only)
    #[arg(long)]
    dedicated_server: bool,

    /// Online Mode: local or sanasol
    #[arg(long)]
    online_mode: Option<String>,

    /// Update Branch: release or pre-release
    #[arg(long)]
    branch: Option<String>,

    /// Game Version: latest, 5, etc.
    #[arg(long)]
    game_version: Option<String>,

    /// Server Args
    #[arg(long)]
    server_args: Option<String>,

    /// Java Exec Args
    #[arg(long)]
    java_exec_args: Option<String>,
}

// Add this function to detect if we are the proxy
fn is_running_as_java_proxy() -> bool {
    // 0. Safe fallback: Environment variable
    if std::env::var("RUSTALE_IS_PROXY").is_ok() {
        return true;
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(name) = exe.file_stem() {
            let name_str = name.to_string_lossy().to_lowercase();

            // 1. Explicit name
            if name_str.contains("rustale_proxy") {
                return true;
            }

            // 2. Impostor detection (Java Hijack)
            // If we are called "java" and "java_original" exists next to us, we are the proxy.
            if name_str == "java" {
                if let Some(dir) = exe.parent() {
                    let original_name = if cfg!(windows) {
                        "java_original.exe"
                    } else {
                        "java_original"
                    };
                    if dir.join(original_name).exists() {
                        return true;
                    }
                }
            }
        }
    }
    false
}

pub fn main() -> iced::Result {
    #[cfg(windows)]
    {
        // Si hay argumentos de consola, reconectamos la salida estándar
        let args: Vec<String> = std::env::args().collect();
        if args
            .iter()
            .any(|a| a == "--dedicated-server" || a == "--help" || a == "-h")
        {
            use windows_sys::Win32::System::Console::{
                ATTACH_PARENT_PROCESS, AllocConsole, AttachConsole,
            };
            unsafe {
                // Intenta adjuntarse a la consola desde donde se lanzó (PowerShell)
                if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
                    // Si falla (ej. doble click), crea una ventana nueva negra
                    AllocConsole();
                }
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Err(e) = gtk::init() {
            eprintln!("Failed to initialize GTK: {}", e);
        }
    }

    // 2. NORMAL MODE: Launcher GUI
    let args = Args::parse();

    // 1. Determine if we should start in Quickplay mode
    // True if it comes from an argument OR if it's in the config file
    let config_initialization_mode = config::load_initialization_config_sync();
    let (width, height) = config::load_width_height();
    let is_quickplay = args.quickplay || config_initialization_mode.quickplay;

    // 1. PROXY MODE: Intercept server execution
    if is_running_as_java_proxy() {
        if let Err(e) = util::run_java_proxy_logic(config_initialization_mode.online_mode) {
            eprintln!("Java Proxy Error: {}", e);
            std::process::exit(1);
        }
        std::process::exit(0);
    }

    if args.dedicated_server {
        // Inicializar runtime básico para el server (sin UI)
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime");

        rt.block_on(async {
            // Cargar configuración (fusiona archivo + CLI)
            let config = server::config::load_or_create(&args).await;

            if let Err(e) = server::runner::run_server_flow(config).await {
                eprintln!("Server Error: {}", e);
                std::process::exit(1);
            }
        });
        std::process::exit(0);
    }

    iced::application(
        move || RusTale::new(is_quickplay),
        RusTale::update,
        RusTale::view,
    )
    .theme(RusTale::theme)
    .subscription(RusTale::subscription)
    .title(RusTale::title)
    .window(iced::window::Settings {
        size: iced::Size::new(width, height),
        min_size: Some(iced::Size::new(480.0, 390.0)),
        resizable: true,
        icon: iced::window::icon::from_file_data(include_bytes!("../assets/logo.png"), None).ok(),

        // 2. Set the initial visibility CORRECTLY from the start
        visible: !is_quickplay,

        position: iced::window::Position::Centered,
        exit_on_close_request: false,

        ..Default::default()
    })
    .run()
}

#[derive(Debug, Clone)]
pub enum Message {
    None,
    Initialize,
    Mods(ModsMessage),                        // New type of wrapper message
    ModsLoaded(Result<Vec<ModInfo>, String>), // Result of the load
    ConfigLoaded(ProfilesConfig, GameSettings, Localization),
    LanguageChangedInSettings(String),
    News(NewsMessage),
    Settings(SettingsMessage),
    OpenSettings,
    CloseSettings,
    SaveSettings(GameSettings),
    CheckStatus,
    DryRunFinished(GameSettings, LauncherStatus, Option<i32>),
    StartGame,
    DownloadProgress {
        progress: f32,
        sub_progress: f32,
        speed: String,
    },
    GameLaunched(Result<(), String>),
    GameStopped,
    OpenFolder,
    RequestVersionCheck(String),
    VersionsReceived(Vec<i32>),
    RequestDeleteVersion(u32),
    OpenVersionFolder(u32),
    InstalledVersionsReceived(Vec<(i32, bool)>),
    BackgroundLoaded(Result<image::Handle, String>),
    ProfileSelected(Profile),
    AddProfile,
    EditProfile(String),
    DeleteProfile(String),
    ProfileNameChanged(String),
    SaveProfileName,
    CancelProfileEdit,
    DownloadError(String),
    ToggleProfileDropdown,
    TrayEvent(tray_icon::TrayIconEvent),
    TrayMenuEvent(tray_icon::menu::MenuEvent),
    ToggleWindowVisibility,
    AppExit,
    CloseRequested,
    WindowResized(Size),
    OpenMods,
    ModsLoadedComplex(Result<(Vec<ModInfo>, Vec<PatchManifest>), String>), // Result of the load
    LauncherUpdate(updater::UpdaterMessage),
}

struct RusTale {
    profiles: ProfilesConfig,
    settings: GameSettings,
    status: LauncherStatus,
    news_section: NewsSection,
    settings_state: SettingsState,
    download_progress: f32,
    sub_progress: f32,
    status_text: String,
    error: Option<String>,
    running_game: Option<(GameSettings, String, String, Option<i32>, LauncherStatus)>, // (Settings, Name, ID/UUID, TargetVersion)
    bg_handle: Option<image::Handle>,
    editing_profile: Option<(Option<String>, String)>, // (ID, Name) - None ID means new profile
    profile_dropdown_open: bool,
    latest_version: Option<i32>,
    available_versions: Vec<i32>, // CAMBIO: Caché persistente de versiones
    paths: GamePaths,             // Centralized path management
    api_client: reqwest::Client,  // For news, auth, version check
    download_client: reqwest::Client, // For JRE, PWR, Assets
    localization: Localization,
    is_quickplay_mode: bool,
    is_window_visible: bool,
    window_size: Size,
    tray_icon: Option<tray_icon::TrayIcon>, // Store tray icon to rebuild menu dynamically
    mods_state: ModsState,                  // Modal state
    local_server_stop_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl RusTale {
    fn new(quickplay: bool) -> (Self, Task<Message>) {
        let initial_settings = GameSettings::default();
        let base_dir = config::get_app_dir();
        let paths = GamePaths::new(base_dir);

        let (width, height) = config::load_width_height();

        // 1. API CLIENT: Fast, fails quickly if no response
        let api_client = reqwest::Client::builder()
            .user_agent("RusTale/0.0.1")
            .timeout(std::time::Duration::from_secs(15)) // 15s is enough for JSON
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        // 2. DOWNLOAD CLIENT: Robust, "heavy lifting"
        let download_client = reqwest::Client::builder()
            .user_agent("RusTale-Downloader/0.0.1")
            // Connection timeout: fails if not connected in 30s
            .connect_timeout(std::time::Duration::from_secs(30))
            // Keepalive to maintain open TCP connection
            .tcp_keepalive(std::time::Duration::from_secs(60))
            // Force HTTP/1.1 for stability in large downloads (avoids HTTP2 bugs in some CDNs)
            .http1_only()
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let client_for_bg = api_client.clone();

        // --- BACKGROUND OPTIMIZATION ---
        // Background URL
        let bg_url = "https://hytale.com/static/images/backgrounds/content-upper-new-1920.jpg";

        // 1. Try fast synchronous load from cache (0ms latency)
        let initial_bg = util::image_cache::load_image_sync_if_exists(bg_url);

        // 2. If not in cache, create async download task
        //    (only happens the first time the launcher is opened)
        let bg_task = if initial_bg.is_some() {
            Task::none() // Already have the image, no need to download
        } else {
            Task::perform(
                async move {
                    util::image_cache::load_image(&client_for_bg, bg_url)
                        .await
                        .map_err(|e| e.to_string())
                },
                Message::BackgroundLoaded,
            )
        };

        // --- TRAY SYSTEM ---
        // Create initial tray icon with "Start Game" option
        let tray_icon = Self::create_tray_icon(false);

        (
            Self {
                profiles: ProfilesConfig::default(),
                settings: initial_settings.clone(),
                status: LauncherStatus::Checking,
                news_section: NewsSection::new(),
                settings_state: SettingsState::new(initial_settings),
                download_progress: 0.0,
                sub_progress: 0.0,
                status_text: "Initializing...".to_string(),
                error: None,
                running_game: None,
                bg_handle: initial_bg, // Assign the handle immediately (can be Some or None)
                editing_profile: None,
                profile_dropdown_open: false,
                latest_version: None,
                available_versions: Vec::new(), // Initialize empty
                paths,
                api_client,
                download_client,
                localization: Localization::new(),
                is_quickplay_mode: quickplay,
                is_window_visible: !quickplay,
                window_size: Size::new(width, height),
                tray_icon,
                mods_state: ModsState::new(),
                local_server_stop_tx: None,
            },
            Task::batch(vec![
                Task::done(Message::Initialize),
                bg_task, // Use the conditional task (Task::none() if already loaded, or Task::perform if we need to download)
            ]),
        )
    }

    fn theme(&self) -> Theme {
        Theme::Dark
    }

    fn title(&self) -> String {
        self.localization.t("launcher.title").to_string()
    }

    fn subscription(&self) -> Subscription<Message> {
        let game_runner =
            if let Some((settings, name, uuid, target_ver, trigger_status)) = &self.running_game {
                // Ahora usamos trigger_status en lugar de self.status
                // Esto garantiza que si pulsamos "ACTUALIZAR", la política sea NetworkUpdate
                let policy = match trigger_status {
                    LauncherStatus::NeedsInstall | LauncherStatus::NeedsUpdate => {
                        InstallPolicy::NetworkUpdate
                    }
                    _ => {
                        // Si estaba en Ready o cualquier otro, verificamos rápido (Offline)
                        InstallPolicy::OfflineVerify
                    }
                };

                game::runner::run(
                    settings.clone(),
                    name.clone(),
                    uuid.clone(),
                    self.download_client.clone(),
                    *target_ver,
                    policy,
                )
            } else {
                Subscription::none()
            };

        let tray_sub = Subscription::run(tray_events);
        let menu_sub = Subscription::run(menu_events);

        let window_sub = window::events().map(|(_id, event)| match event {
            window::Event::Resized(size) => Message::WindowResized(size),
            window::Event::CloseRequested => Message::CloseRequested,
            _ => Message::None,
        });

        Subscription::batch(vec![game_runner, tray_sub, menu_sub, window_sub])
    }

    fn reconcile_local_server(&mut self) {
        let needs_server = self.settings.online_fix_mode == config::OnlineFixMode::Local;
        let is_running = self.local_server_stop_tx.is_some();

        if needs_server && !is_running {
            // INICIAR SERVIDOR
            let port = util::get_saved_port();
            let base_dir = config::get_app_dir();
            let paths = game::GamePaths::new(base_dir.clone());

            // Usamos la carpeta del perfil actual o default
            let channel = &self.settings.channel;
            // Intentar adivinar la carpeta del juego para los assets
            let game_dir = paths.version_dir(channel, "latest");

            let username = self.profiles.get_current_profile_name();
            let uuid = self.profiles.current_profile.clone();

            let (tx, rx) = tokio::sync::oneshot::channel();
            self.local_server_stop_tx = Some(tx);

            // Spawnear
            tokio::spawn(async move {
                // Primero check si ya corre
                if !game::server::is_server_alive(port).await {
                    println!("[App] Starting background Auth Server on {}", port);
                    let _ = game::server::start_server(username, uuid, game_dir, rx, port).await;
                } else {
                    println!("[App] Auth Server already active on {}. Attached.", port);
                }
            });
        } else if !needs_server && is_running {
            // DETENER SERVIDOR
            if let Some(tx) = self.local_server_stop_tx.take() {
                let _ = tx.send(());
                println!("[App] Background Auth Server stopped.");
            }
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        if self.settings_state.is_open {
            match &message {
                Message::Settings(_)
                | Message::LanguageChangedInSettings(_)
                | Message::CloseSettings
                | Message::SaveSettings(_)
                | Message::VersionsReceived(_)
                | Message::InstalledVersionsReceived(_)
                | Message::RequestVersionCheck(_)
                | Message::RequestDeleteVersion(_)
                | Message::OpenVersionFolder(_)
                | Message::CloseRequested
                | Message::WindowResized(_)
                | Message::AppExit => {}
                _ => return Task::none(),
            }
        }

        match message {
            Message::Mods(msg) => {
                let base_dir = config::get_app_dir();
                let client = self.api_client.clone();
                let settings = self.settings.clone();

                self.mods_state
                    .update(msg, client, base_dir, settings)
                    .map(Message::Mods)
            }
            Message::OpenMods => Task::done(Message::Mods(ModsMessage::OpenMods)),
            Message::ModsLoaded(res) => Task::done(Message::Mods(ModsMessage::ModsLoaded(res))),
            Message::ModsLoadedComplex(res) => {
                Task::done(Message::Mods(ModsMessage::ModsLoadedComplex(res)))
            }

            Message::Initialize => Task::perform(
                async {
                    let _ = app::initialize().await;
                    let p = config::load_profiles().await;
                    let s = config::load_settings().await;

                    // --- LANGUAGE LOGIC ---
                    let mut loc = Localization::new();

                    // 1. Scan names (fast, low RAM)
                    loc.load_available_languages();

                    // 2. Load saved language (or English if first time)
                    loc.load_language(&s.language);

                    (p, s, loc)
                },
                |(p, s, loc)| Message::ConfigLoaded(p, s, loc),
            ),
            Message::ConfigLoaded(p, s, loc) => {
                self.profiles = p;
                self.settings = s.clone();
                self.settings_state.temp_settings = s;
                self.localization = loc;

                // NUEVO: Asegurar servidor si es necesario
                self.reconcile_local_server();

                // --- AUTO-QUICKPLAY LOGIC ---
                // Activated if passed by CLI (--quickplay) OR if enabled in permanent settings
                if self.is_quickplay_mode || self.settings.quickplay {
                    self.is_quickplay_mode = true;
                    self.is_window_visible = false;
                }

                let news_task = if self.settings.enable_news {
                    let client = self.api_client.clone();
                    Task::perform(
                        async move {
                            crate::news::fetch_news(&client)
                                .await
                                .map_err(|e| e.to_string())
                        },
                        |res| Message::News(NewsMessage::NewsLoaded(res)),
                    )
                } else {
                    Task::none()
                };

                // --- UPDATER LOGIC ---
                let update_task = if self.settings.enable_auto_update {
                    let client = self.api_client.clone();
                    Task::perform(
                        async move {
                            match updater::check_for_updates(&client).await {
                                Ok(Some(info)) => updater::UpdaterMessage::UpdateFound(info),
                                Ok(None) => updater::UpdaterMessage::UpdateNotFound,
                                Err(e) => updater::UpdaterMessage::Error(e.to_string()),
                            }
                        },
                        Message::LauncherUpdate,
                    )
                } else {
                    Task::none()
                };

                let mut tasks = vec![Task::done(Message::CheckStatus), news_task, update_task];

                if self.is_quickplay_mode {
                    tasks.push(
                        window::oldest().and_then(|id| window::set_mode(id, window::Mode::Hidden)),
                    );
                }

                Task::batch(tasks)
            }
            Message::LanguageChangedInSettings(lang_id) => {
                // 1. Update the temporary settings state (for when saving)
                self.settings_state.temp_settings.language = lang_id.clone();

                // 2. Load the new language instantly
                // This replaces the previous JSON in RAM
                self.localization.load_language(&lang_id);

                // 3. Iced will automatically repaint the UI with the new texts
                Task::none()
            }
            Message::BackgroundLoaded(res) => {
                if let Ok(handle) = res {
                    self.bg_handle = Some(handle);
                }
                Task::none()
            }
            Message::CheckStatus => {
                self.status = LauncherStatus::Checking;
                self.status_text = self.localization.t("launcher.status.checking").to_string();

                let settings = self.settings.clone();
                let settings_for_closure = settings.clone();
                let paths = self.paths.clone();
                let client = self.api_client.clone();

                // CAMBIO: Pasamos el latest_version que ya tenemos en memoria (si existe)
                let cached_version = self.latest_version;

                Task::perform(
                    async move {
                        game::calculate_status(&client, &settings, &paths, cached_version).await
                    },
                    move |(status, latest)| {
                        Message::DryRunFinished(settings_for_closure, status, latest)
                    },
                )
            }
            Message::DryRunFinished(settings, status, latest) => {
                self.settings = settings;
                self.status = status;

                // CAMBIO: Si recibimos un dato remoto nuevo, actualizamos caché y generamos lista
                if let Some(ver) = latest {
                    self.latest_version = Some(ver);
                    if ver > 0 {
                        let mut versions = Vec::new();
                        for i in (1..=ver).rev().take(50) {
                            // Limitamos a 50 para no saturar UI
                            versions.push(i);
                        }
                        self.available_versions = versions;
                    }
                }

                self.status_text = match self.status {
                    LauncherStatus::NeedsInstall => {
                        self.localization.t("launcher.play").to_string()
                    }
                    LauncherStatus::NeedsUpdate => {
                        self.localization.t("launcher.update").to_string()
                    }
                    _ => self.localization.t("launcher.status.ready").to_string(),
                };

                // QUICKPLAY LOGIC
                if self.is_quickplay_mode {
                    match self.status {
                        LauncherStatus::Ready | LauncherStatus::Playing => {
                            return Task::done(Message::StartGame);
                        }
                        LauncherStatus::NeedsUpdate | LauncherStatus::NeedsInstall => {
                            // In Quickplay, force download/update
                            return Task::done(Message::StartGame);
                        }
                        _ => {
                            // If there's an error or something weird, show the window
                            self.is_quickplay_mode = false;
                            self.is_window_visible = true;
                            return window::oldest().and_then(|id| {
                                Task::batch(vec![
                                    window::set_mode(id, window::Mode::Windowed),
                                    window::gain_focus(id),
                                ])
                            });
                        }
                    }
                }

                Task::none()
            }
            Message::StartGame => {
                let mut tasks = Vec::new();

                if self.status == LauncherStatus::Playing {
                    self.running_game = None;
                    self.status = LauncherStatus::Ready;
                } else {
                    let player_name = self.profiles.get_current_profile_name();
                    let player_uuid = self.profiles.current_profile.clone();
                    let settings = self.settings.clone();
                    let target_ver = self.latest_version;

                    // CAPTURA: Guardamos el estado actual (ej: NeedsUpdate) antes de cambiarlo
                    let trigger_status = self.status.clone();

                    if self.is_quickplay_mode {
                        self.is_window_visible = false;
                        tasks.push(
                            window::oldest()
                                .and_then(|id| window::set_mode(id, window::Mode::Hidden)),
                        );
                    }

                    // Guardamos el trigger_status en la tupla
                    self.running_game = Some((
                        settings,
                        player_name,
                        player_uuid,
                        target_ver,
                        trigger_status,
                    ));
                    self.status = LauncherStatus::Busy;
                }
                Task::batch(tasks)
            }

            Message::DownloadProgress {
                progress,
                sub_progress,
                speed,
            } => {
                self.status = LauncherStatus::Downloading;
                self.download_progress = progress;
                self.sub_progress = sub_progress;
                self.status_text = speed;
                Task::none()
            }
            Message::GameLaunched(res) => {
                match res {
                    Ok(_) => {
                        self.status = LauncherStatus::Playing;
                        self.status_text =
                            self.localization.t("launcher.status.playing").to_string();

                        // Rebuild tray menu to show "Stop Game"
                        self.rebuild_tray_menu();

                        if self.settings.minimize_on_play || self.is_quickplay_mode {
                            self.is_window_visible = false;

                            return window::oldest()
                                .and_then(|id| window::set_mode(id, window::Mode::Hidden));
                        }
                    }
                    Err(e) => {
                        self.status = LauncherStatus::Ready;
                        self.error = Some(e);
                        self.running_game = None;

                        // If we fail in Quickplay, show the window so the user knows what happened
                        if self.is_quickplay_mode {
                            self.is_quickplay_mode = false;
                            self.is_window_visible = true;
                            return window::oldest().and_then(|id| {
                                Task::batch(vec![
                                    window::set_mode(id, window::Mode::Windowed),
                                    window::gain_focus(id),
                                ])
                            });
                        }
                    }
                }
                Task::none()
            }

            Message::GameStopped => {
                self.status = LauncherStatus::Ready;
                self.status_text = self.localization.t("launcher.status.ready").to_string();
                self.running_game = None;

                // Rebuild tray menu to show "Start Game"
                self.rebuild_tray_menu();

                // Modificamos la lógica aquí para mayor seguridad:
                // Solo salimos si es quickplay Y la ventana no está visible.
                if self.is_quickplay_mode && !self.is_window_visible {
                    return self.save_and_exit();
                }

                if self.settings.minimize_on_play {
                    self.is_window_visible = true;
                    return window::oldest().and_then(|id| {
                        Task::batch(vec![
                            window::set_mode(id, window::Mode::Windowed),
                            window::gain_focus(id),
                        ])
                    });
                }

                // Si veníamos de quickplay pero el usuario abrió la ventana,
                // nos aseguramos de que la ventana se quede visible y activa.
                if self.is_window_visible {
                    return window::oldest().and_then(|id| {
                        Task::batch(vec![
                            window::set_mode(id, window::Mode::Windowed),
                            window::gain_focus(id),
                        ])
                    });
                }

                Task::none()
            }
            Message::News(msg) => {
                if !self.settings.enable_news {
                    return Task::none();
                }
                self.news_section.update(msg, self.api_client.clone())
            }
            Message::LauncherUpdate(sub_msg) => match sub_msg {
                updater::UpdaterMessage::CheckForUpdates => {
                    let client = self.api_client.clone();
                    Task::perform(
                        async move {
                            match updater::check_for_updates(&client).await {
                                Ok(Some(info)) => updater::UpdaterMessage::UpdateFound(info),
                                Ok(None) => updater::UpdaterMessage::UpdateNotFound,
                                Err(e) => updater::UpdaterMessage::Error(e.to_string()),
                            }
                        },
                        Message::LauncherUpdate,
                    )
                }
                updater::UpdaterMessage::UpdateFound(info) => {
                    self.status_text = format!("Update found: v{}", info.tag_name);
                    if let Some(asset) = info.assets.first() {
                        let url = asset.browser_download_url.clone();
                        Task::done(Message::LauncherUpdate(
                            updater::UpdaterMessage::StartUpdate(url),
                        ))
                    } else {
                        eprintln!("Update found but no assets!");
                        Task::none()
                    }
                }
                updater::UpdaterMessage::StartUpdate(url) => {
                    self.status = LauncherStatus::Busy;
                    self.status_text = "Updating Launcher...".to_string();

                    let client = self.download_client.clone();
                    Task::perform(
                        async move {
                            updater::perform_update(client, url)
                                .await
                                .map_err(|e| e.to_string())
                        },
                        |res| match res {
                            Ok(_) => {
                                Message::LauncherUpdate(updater::UpdaterMessage::UpdateFinished)
                            }
                            Err(e) => Message::LauncherUpdate(updater::UpdaterMessage::Error(e)),
                        },
                    )
                }
                updater::UpdaterMessage::UpdateFinished => {
                    std::process::exit(0);
                }
                updater::UpdaterMessage::UpdateNotFound => {
                    // console log
                    Task::none()
                }
                updater::UpdaterMessage::Error(e) => {
                    eprintln!("Update check failed: {}", e);
                    Task::none()
                }
                _ => Task::none(),
            },

            Message::OpenSettings => {
                self.settings_state.open(self.settings.clone());

                // CAMBIO IMPORTANTE:
                // 1. No llamamos a RequestVersionCheck (evita HTTP).
                // 2. Inyectamos las versiones cacheadas directamente.
                self.settings_state.available_versions = self.available_versions.clone();
                self.settings_state.is_loading_versions = false;

                let channel = self.settings.channel.clone();
                Task::perform(
                    async move {
                        let base_dir = config::get_app_dir();
                        game::install::get_installed_versions(&base_dir, &channel).await
                    },
                    Message::InstalledVersionsReceived,
                )
            }
            Message::CloseSettings => {
                self.settings_state.is_open = false;
                Task::none()
            }
            Message::SaveSettings(new_settings) => {
                let old_settings = self.settings.clone();
                self.settings = new_settings.clone();
                let s = new_settings.clone();

                // Reload paths in case bootstrap was changed
                let base_dir = config::get_app_dir();
                self.paths = GamePaths::new(base_dir);

                // NUEVO: Reconciliar estado del servidor local
                self.reconcile_local_server();

                let news_action = if s.enable_news && !old_settings.enable_news {
                    Task::done(Message::News(NewsMessage::LoadNews))
                } else {
                    Task::none()
                };

                Task::batch(vec![
                    Task::perform(async move { config::save_settings(&s).await }, |_| {
                        Message::CheckStatus
                    }),
                    news_action,
                ])
            }
            Message::Settings(SettingsMessage::BrowseInstallPath) => {
                let title = self.localization.t("dialog.select_folder").to_string();
                Task::perform(
                    async move {
                        rfd::AsyncFileDialog::new()
                            .set_title(title)
                            .pick_folder()
                            .await
                            .map(|f| f.path().to_path_buf())
                    },
                    |res| {
                        if let Some(path) = res {
                            Message::Settings(SettingsMessage::PathSelected(path))
                        } else {
                            Message::None
                        }
                    },
                )
            }
            Message::Settings(msg) => {
                if let Some(m) = self.settings_state.update(msg) {
                    Task::done(m)
                } else {
                    Task::none()
                }
            }
            Message::RequestVersionCheck(chan) => {
                let client = self.api_client.clone();
                Task::perform(
                    async move {
                        // Aquí cache_remote es None implícitamente porque find_latest_version siempre busca
                        let v = game::patcher::find_latest_version(&client, &chan)
                            .await
                            .unwrap_or(0);
                        let base_dir = config::get_app_dir();
                        let installed =
                            game::install::get_installed_versions(&base_dir, &chan).await;
                        (v, installed)
                    },
                    |(v, _installed)| {
                        let mut versions = Vec::new();
                        if v > 0 {
                            for i in (1..=v).rev().take(50) {
                                versions.push(i);
                            }
                        }
                        Message::VersionsReceived(versions)
                    },
                )
            }
            Message::VersionsReceived(v) => {
                self.settings_state.available_versions = v.clone();

                // Opcional: Actualizar también el caché global si el canal coincide
                if self.settings_state.temp_settings.channel == self.settings.channel {
                    self.available_versions = v;
                }

                self.settings_state.is_loading_versions = false;

                // If we arrived here from channel change, we should also check installed versions again
                let channel = self.settings_state.temp_settings.channel.clone();
                Task::perform(
                    async move {
                        let base_dir = config::get_app_dir();
                        game::install::get_installed_versions(&base_dir, &channel).await
                    },
                    Message::InstalledVersionsReceived,
                )
            }
            Message::InstalledVersionsReceived(v) => {
                self.settings_state.installed_versions = v;
                Task::none()
            }
            Message::RequestDeleteVersion(v) => {
                let channel = self.settings_state.temp_settings.channel.clone();
                Task::perform(
                    async move {
                        let base_dir = config::get_app_dir();
                        let _ = game::install::delete_version(&base_dir, &channel, v as i32).await;
                        game::install::get_installed_versions(&base_dir, &channel).await
                    },
                    Message::InstalledVersionsReceived,
                )
            }
            Message::DownloadError(err) => {
                self.status = LauncherStatus::Ready;
                self.error = Some(err);

                // --- NEW: Clean up running game ---
                // This ensures Iced knows there's no active subscription
                self.running_game = None;
                // ---------------------------------

                Task::none()
            }
            Message::OpenFolder => {
                util::open_game_folder();
                Task::none()
            }
            Message::OpenVersionFolder(v) => {
                let channel = self.settings.channel.clone();

                Task::perform(
                    async move {
                        let base_dir = config::get_app_dir();
                        let paths = game::GamePaths::new(base_dir.clone());

                        // If v is 0, path.rs already knows to resolve to ".../latest"
                        let folder = paths.version_dir(&channel, &v.to_string());

                        if folder.exists() {
                            util::open_path(folder);
                        }
                    },
                    |_| Message::None,
                )
            }
            Message::ProfileSelected(_)
            | Message::AddProfile
            | Message::EditProfile(_)
            | Message::DeleteProfile(_)
            | Message::ProfileNameChanged(_)
            | Message::SaveProfileName
            | Message::CancelProfileEdit
            | Message::ToggleProfileDropdown => self.handle_profile_message(message),
            Message::TrayEvent(evt) => {
                if let tray_icon::TrayIconEvent::Click {
                    button: tray_icon::MouseButton::Left,
                    ..
                } = evt
                {
                    self.is_window_visible = !self.is_window_visible;
                    let is_visible = self.is_window_visible;

                    // Si el usuario muestra la ventana manualmente, desactivamos el modo quickplay
                    if is_visible {
                        self.is_quickplay_mode = false;
                    }

                    return window::oldest().and_then(move |id| {
                        if is_visible {
                            Task::batch(vec![
                                window::set_mode(id, window::Mode::Windowed),
                                window::gain_focus(id),
                            ])
                        } else {
                            window::set_mode(id, window::Mode::Hidden)
                        }
                    });
                }
                Task::none()
            }
            Message::TrayMenuEvent(evt) => match evt.id.as_ref() {
                "quit" => return Task::done(Message::AppExit),
                "start" => {
                    if self.status == LauncherStatus::Ready {
                        Task::done(Message::StartGame)
                    } else {
                        Task::none()
                    }
                }
                "stop" => {
                    if self.status == LauncherStatus::Playing {
                        Task::done(Message::GameStopped)
                    } else {
                        Task::none()
                    }
                }
                "show_hide" => {
                    self.is_window_visible = !self.is_window_visible;
                    let is_visible = self.is_window_visible;

                    // Igual aquí, si muestra la ventana, ya no es quickplay puro
                    if is_visible {
                        self.is_quickplay_mode = false;
                    }

                    window::oldest().and_then(move |id| {
                        if is_visible {
                            Task::batch(vec![
                                window::set_mode(id, window::Mode::Windowed),
                                window::gain_focus(id),
                            ])
                        } else {
                            window::set_mode(id, window::Mode::Hidden)
                        }
                    })
                }
                _ => Task::none(),
            },
            Message::ToggleWindowVisibility => {
                self.is_window_visible = !self.is_window_visible;
                let is_visible = self.is_window_visible;

                window::oldest().and_then(move |id| {
                    if is_visible {
                        Task::batch(vec![
                            window::set_mode(id, window::Mode::Windowed),
                            window::gain_focus(id),
                        ])
                    } else {
                        window::set_mode(id, window::Mode::Hidden)
                    }
                })
            }
            Message::CloseRequested => {
                if self.settings.minimize_to_tray {
                    self.is_window_visible = false;
                    window::oldest().and_then(|id| window::set_mode(id, window::Mode::Hidden))
                } else {
                    self.save_and_exit()
                }
            }
            Message::WindowResized(size) => {
                self.window_size = size;

                // Actualizamos los settings en memoria
                self.settings.width = size.width as u32;
                self.settings.height = size.height as u32;

                // Sincronizamos también el estado temporal para que, si el modal está abierto,
                // no se sobrescriba con la resolución vieja al pulsar "Save".
                self.settings_state.temp_settings.width = size.width as u32;
                self.settings_state.temp_settings.height = size.height as u32;

                // REQUERIMIENTO: Si se reduce mucho, deshabilitar noticias en settings
                if self.settings.enable_news && size.width < 750.0 {
                    self.settings.enable_news = false;

                    // Sincronizamos también el estado temporal de settings si el modal está abierto
                    self.settings_state.temp_settings.enable_news = false;
                }

                Task::none()
            }
            Message::AppExit => self.save_and_exit(),
            Message::None => Task::none(),
        }
    }

    fn handle_profile_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ProfileSelected(profile) => {
                self.profiles.current_profile = profile.id;
                self.profile_dropdown_open = false;
                let profiles = self.profiles.clone();
                Task::perform(
                    async move { config::save_profiles(&profiles).await },
                    |_| Message::None,
                )
            }
            Message::AddProfile => {
                self.editing_profile = Some((None, "".to_string()));
                Task::none()
            }
            Message::EditProfile(id) => {
                if let Some(p) = self.profiles.profiles.iter().find(|p| p.id == id) {
                    self.editing_profile = Some((Some(p.id.clone()), p.name.clone()));
                }
                Task::none()
            }
            Message::DeleteProfile(id) => {
                self.profiles.delete_profile(&id);
                let profiles = self.profiles.clone();
                Task::perform(
                    async move { config::save_profiles(&profiles).await },
                    |_| Message::None,
                )
            }
            Message::ProfileNameChanged(name) => {
                if let Some((id, _)) = &self.editing_profile {
                    self.editing_profile = Some((id.clone(), name));
                }
                Task::none()
            }
            Message::SaveProfileName => {
                if let Some((id, name)) = self.editing_profile.take() {
                    if !name.trim().is_empty() {
                        if let Some(profile_id) = id {
                            self.profiles.update_profile(&profile_id, name);
                        } else {
                            self.profiles.add_profile(name);
                        }
                        let profiles = self.profiles.clone();
                        Task::perform(
                            async move { config::save_profiles(&profiles).await },
                            |_| Message::None,
                        )
                    } else {
                        Task::none()
                    }
                } else {
                    Task::none()
                }
            }
            Message::CancelProfileEdit => {
                self.editing_profile = None;
                Task::none()
            }
            Message::ToggleProfileDropdown => {
                self.profile_dropdown_open = !self.profile_dropdown_open;
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let left_column_content = column![
            profile_card::view(
                &self.profiles,
                &self.editing_profile,
                self.profile_dropdown_open,
                &self.localization,
            ),
            Space::new().height(Length::Fill),
            control_section::view(
                &self.status,
                &self.settings,
                self.download_progress,
                self.sub_progress,
                &self.status_text,
                &self.localization,
            ),
        ]
        .spacing(20);

        let show_news = self.settings.enable_news && self.window_size.width > 750.0;

        let main_content: Element<'_, Message> = if show_news {
            let left_column = container(left_column_content)
                .width(Length::FillPortion(1))
                .height(Length::Fill)
                .padding(30)
                .style(theme::glass_container);

            let right_column = container(
                self.news_section
                    .view(&self.localization)
                    .map(Message::News),
            )
            .width(Length::FillPortion(2))
            .height(Length::Fill)
            .padding(30);

            row![left_column, right_column]
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(20)
                .spacing(20)
                .into()
        } else {
            // MODO COMPACTO / VENTANA PEQUEÑA
            // Usamos Length::Fill para que ocupe todo el ancho disponible
            // Reducimos el padding para aprovechar espacio en ventanas muy pequeñas (480px)
            let padding = if self.window_size.width < 500.0 {
                10
            } else {
                30
            };

            let left_column = container(left_column_content)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(padding) // Padding dinámico
                .style(theme::glass_container);

            // Centramos la columna si hay mucho espacio, o la llenamos si es pequeña
            if self.window_size.width > 500.0 {
                row![
                    Space::new().width(Length::Fill),
                    container(left_column).width(400.0), // Max width visual
                    Space::new().width(Length::Fill)
                ]
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(20)
                .into()
            } else {
                container(left_column)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .padding(10) // Margen externo pequeño
                    .into()
            }
        };

        let bg: Element<'_, Message> = if let Some(handle) = &self.bg_handle {
            image(handle.clone())
                .width(Length::Fill)
                .height(Length::Fill)
                .content_fit(ContentFit::Cover)
                .into()
        } else {
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_t: &Theme| container::Style {
                    background: Some(iced::Background::Color(Color::from_rgb(0.05, 0.05, 0.05))),
                    ..Default::default()
                })
                .into()
        };

        let final_view = stack![bg, main_content];

        let overlay = if self.settings_state.is_open {
            Some(container(
                self.settings_state
                    .view(&self.localization, self.window_size)
                    .map(Message::Settings),
            ))
        } else if self.mods_state.is_open {
            Some(container(
                self.mods_state
                    .view(&self.localization, self.window_size)
                    .map(Message::Mods),
            ))
        } else {
            None
        };

        let with_settings = if let Some(content) = overlay {
            stack![
                final_view,
                container(content)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(iced::Background::Color(Color::from_rgba(
                            0.0, 0.0, 0.0, 0.8
                        ))),
                        ..Default::default()
                    })
            ]
        } else {
            final_view
        };
        with_settings.into()
    }

    /// Creates a tray icon with the appropriate menu based on game state
    /// is_playing: true if game is currently running
    fn create_tray_icon(is_playing: bool) -> Option<tray_icon::TrayIcon> {
        let tray_menu = Menu::new();

        // Change menu item based on game state
        let game_action = if is_playing {
            MenuItem::with_id("stop", "Stop Game", true, None)
        } else {
            MenuItem::with_id("start", "Start Game", true, None)
        };

        let show_i = MenuItem::with_id("show_hide", "Show/Hide Launcher", true, None);
        let quit_i = MenuItem::with_id("quit", "Quit RusTale", true, None);
        let _ = tray_menu.append_items(&[&game_action, &show_i, &quit_i]);

        let icon = util::icons::load_tray_icon();
        TrayIconBuilder::new()
            .with_menu(Box::new(tray_menu))
            .with_tooltip("RusTale Launcher")
            .with_icon(icon)
            .build()
            .ok()
    }

    /// Rebuilds the tray menu based on current game state
    fn rebuild_tray_menu(&mut self) {
        let is_playing = self.status == LauncherStatus::Playing;
        self.tray_icon = Self::create_tray_icon(is_playing);
    }

    fn save_and_exit(&mut self) -> Task<Message> {
        println!("Guardando settings...");
        self.settings = self.settings_state.temp_settings.clone();

        // Guardamos antes de matar el proceso
        let _ = config::save_settings_sync(&self.settings);
        iced::exit()
    }
}

fn tray_events() -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(
        10,
        |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
            loop {
                if let Ok(event) = tray_icon::TrayIconEvent::receiver().try_recv() {
                    let _ = output.send(Message::TrayEvent(event)).await;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        },
    )
}

fn menu_events() -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(
        10,
        |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
            loop {
                if let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
                    let _ = output.send(Message::TrayMenuEvent(event)).await;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        },
    )
}
