#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;
use futures::SinkExt;
use iced::widget::{Space, column, container, image, mouse_area, row, stack};
use iced::{
    Alignment, Color, ContentFit, Element, Length, Padding, Point, Size, Subscription, Task, Theme,
    clipboard,
    event::{self, Event},
    mouse,
    mouse::Interaction,
    window,
};
use single_instance::SingleInstance;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tray_icon::{
    TrayIconBuilder,
    menu::{Menu, MenuItem},
};

mod app;
mod config;
mod game;
mod java;
mod java_detection;
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
use crate::ui::{control_section, lsd_shader, profile_card}; // Import the struct

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

    /// Tunnel Provider
    #[arg(long)]
    tunnel: Option<String>,
}

// Add this function to detect if we are the proxy
fn is_running_as_java_proxy() -> bool {
    // 0. Safe fallback: Environment variable (Set by Runner)
    if std::env::var("RUSTALE_IS_PROXY").is_ok() {
        return true;
    }
    // Checking for AURORA_MODE is a strong signal we are the proxy
    if std::env::var("AURORA_MODE").is_ok() {
        return true;
    }

    for arg in std::env::args().skip(1) {
        if arg.starts_with("-X") || arg.starts_with("-D") || arg == "-jar" || arg == "-cp" {
            return true;
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(name) = exe.file_stem() {
            let name_str = name.to_string_lossy().to_lowercase();

            if name_str.contains("rustale_proxy") {
                return true;
            }

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
    if is_running_as_java_proxy() {
        let mode_env = std::env::var("AURORA_MODE").unwrap_or_default();
        let mode = match mode_env.as_str() {
            "sanasol" => config::OnlineFixMode::Sanasol,
            _ => config::OnlineFixMode::Local,
        };

        if let Err(e) = util::run_java_proxy_logic(mode) {
            // Write to a panic log file just in case stdout is swallowed
            if let Ok(mut f) = std::fs::File::create("proxy_panic.log") {
                use std::io::Write;
                let _ = writeln!(f, "Proxy Critical Error: {}", e);
            }
            eprintln!("Java Proxy Error: {}", e);
            std::process::exit(1);
        }
        std::process::exit(0);
    }
    // [GPU OPTIMIZATION 1] ----------------------------
    // Forzamos a WGPU a usar el modo de alto rendimiento antes de inicializar nada.
    // Esto evita que use la integrada o renderizado por software (WARP/LLVMpipe).
    unsafe { std::env::set_var("WGPU_POWER_PREF", "high") };

    #[cfg(windows)]
    {
        let raw_args: Vec<String> = std::env::args().collect();
        if raw_args
            .iter()
            .any(|a| a == "--dedicated-server" || a == "--help" || a == "-h")
        {
            use windows_sys::Win32::System::Console::{
                ATTACH_PARENT_PROCESS, AllocConsole, AttachConsole,
            };
            unsafe {
                if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
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

    let args = Args::parse();

    let lock_name = if args.dedicated_server {
        "RusTaleServer_Lock"
    } else {
        "RusTaleLauncher_Lock"
    };

    let instance = SingleInstance::new(lock_name).unwrap();

    if !instance.is_single() {
        eprintln!("===================================================");
        eprintln!("ERROR: There's already an instance of RusTale running.");
        eprintln!("If you don't see the window, check Task Manager");
        eprintln!("and close 'rustale.exe' or 'java.exe'.");
        eprintln!("===================================================");
        std::process::exit(1);
    }

    let config_initialization_mode = config::load_initialization_config_sync();
    let (width, height) = config::load_width_height();
    let is_quickplay = args.quickplay || config_initialization_mode.quickplay;

    if args.dedicated_server {
        // Inicializar runtime basico para el server (sin UI)
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime");

        rt.block_on(async {
            // Cargar configuracion (fusiona archivo + CLI)
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

        // --- CAMBIOS PARA CUSTOM TITLEBAR ---
        visible: !is_quickplay,
        decorations: false, // Desactivar barra nativa
        transparent: true,  // Permitir transparencia real (bordes redondeados/semitransparencia)
        // ------------------------------------
        position: iced::window::Position::Centered,
        exit_on_close_request: false,

        ..Default::default()
    })
    .settings(iced::Settings {
        antialiasing: true, // Mejor borde redondeado
        ..Default::default()
    })
    .scale_factor(|app: &RusTale| app.settings.scale_factor)
    .run()
}

// --- AÑADIR ESTE ENUM ---
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResizeDirection {
    North,
    South,
    East,
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
}

#[derive(Debug, Clone)]
pub enum Message {
    None,
    Tick(std::time::Instant),
    CursorMoved(iced::Point),
    ShaderClicked, // Nuevo mensaje para clic en el shader
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
    RequestRepairVersion(u32),
    RepairFinished(Result<(), String>),
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
    EditProfileUUID(String),
    ProfileUUIDChanged(String),
    SaveProfileUUID,
    CancelProfileUUIDEdit,
    CopyUUID(String),
    GenerateRandomUUID,
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
    RequestMoveData(std::path::PathBuf),
    RequestUseDataLocation(std::path::PathBuf),
    DataMoveStarted,
    DataMoveFinished(Result<std::path::PathBuf, String>),
    MigrationProgress(f32),
    StartMigrationActual(std::path::PathBuf, std::path::PathBuf),
    CancelAction,
    LoadJavaInfo,
    JavaInfoLoaded,
    WindowDrag,
    MinimizeWindow,
    MaximizeWindow,

    // --- NUEVOS MENSAJES PARA REDIMENSIÓN MANUAL ---
    ResizePressed(ResizeDirection),
    ResizeReleased,
    WindowEvent(window::Event),
    // ---------------------------------------------

    // --- NUEVO: Mensaje para cambiar shader ---
    NextShader,

    ServerPatchProgress(f32),
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
    editing_uuid: Option<(String, String)>,
    profile_dropdown_open: bool,
    latest_version: Option<i32>,
    available_versions: Vec<i32>, // CAMBIO: Cache persistente de versiones
    paths: GamePaths,             // Centralized path management
    api_client: reqwest::Client,  // For news, auth, version check
    download_client: reqwest::Client, // For JRE, PWR, Assets
    localization: Localization,
    is_quickplay_mode: bool,
    is_window_visible: bool,
    server_patch_progress: f32,
    show_server_patch_progress: bool,
    window_size: Size,
    tray_icon: Option<tray_icon::TrayIcon>, // Store tray icon to rebuild menu dynamically
    mods_state: ModsState,                  // Modal state
    local_server_stop_tx: Option<tokio::sync::oneshot::Sender<()>>,
    cancellation_token: Arc<AtomicBool>,
    palette: theme::Palette,
    lsd_offset: (f32, f32),
    start_time: std::time::Instant,
    lsd_preview: bool,
    cursor_position: iced::Point, // Rastrear raton para efectos
    last_mouse_move_time: std::time::Instant,
    lsd_enabled_time: Option<std::time::Instant>, // Para activacion progresiva
    is_maximized: bool,
    last_title_click: std::time::Instant,

    // --- NUEVOS CAMPOS PARA REDIMENSIÓN ---
    resizing_direction: Option<ResizeDirection>,
    current_window_size: Size,
    current_window_pos: Point,

    // Guardamos el estado AL MOMENTO DE HACER CLICK
    drag_start_window_pos: Point,
    drag_start_window_size: Size,
    drag_start_mouse_screen_pos: Point, // Mouse absoluto (WindowPos + MousePos)
    // -------------------------------------

    // --- CAMPOS PARA EFECTOS TÁCTILES DEL SHADER ---
    shader_click_intensity: f32,           // Intensidad del pulso actual
    shader_click_time: std::time::Instant, // Tiempo del último clic
    lsd_shader_instance: std::cell::RefCell<Option<lsd_shader::LsdShader>>, // Instancia mutable para llamadas dinámicas
    // ---------------------------------------------

    // NUEVOS CAMPOS PARA TRANSICIÓN DE SHADERS
    active_shader_idx: u32,
    next_shader_idx: u32,
    shader_transition: f32, // 0.0 a 1.0
    total_shaders_available: u32,
    shader_change_timer: f32, // Acumulador para cambio automático
                              // ------------------------------------- // Añadir esto
}

impl RusTale {
    fn new(quickplay: bool) -> (Self, Task<Message>) {
        let initial_settings = GameSettings::default();
        let base_dir = config::get_app_dir();
        let paths = GamePaths::new(base_dir);

        let (width, height) = config::load_width_height();

        // 1. API CLIENT: Fast, fails quickly if no response
        let api_client = reqwest::Client::builder()
            .user_agent(format!("RusTale/{}", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(15)) // 15s is enough for JSON
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        // 2. DOWNLOAD CLIENT: Robust, "heavy lifting"
        let download_client = reqwest::Client::builder()
            .user_agent(format!("RusTale-Downloader/{}", env!("CARGO_PKG_VERSION")))
            // Connection timeout: fails if not connected in 30s
            .connect_timeout(std::time::Duration::from_secs(30))
            // Keepalive to maintain open TCP connection
            .tcp_keepalive(std::time::Duration::from_secs(60))
            // Force HTTP/1.1 for stability in large downloads (avoids HTTP2 bugs in some CDNs)
            .http1_only()
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let client_for_bg = api_client.clone();

        // 1. CARGA INICIAL DE SHADERS CON SEGURIDAD
        // Crea carpeta si no existe (la app.rs initialize es async, esto es pre-load rápido o hazlo en initialize)
        // Recomendado: Llamar build_uber_shader() AQUI

        let total_shaders = std::panic::catch_unwind(|| {
            let shader_code = crate::ui::shader_manager::build_uber_shader();
            lsd_shader::set_global_wgsl(shader_code);
            crate::ui::shader_manager::get_shader_count()
        })
        .unwrap_or_else(|_| {
            eprintln!("[SHADER] Panic during shader initialization! Using safe mode fallback.");
            lsd_shader::set_safe_mode_shader();
            1 // Fallback: 1 shader (safe mode)
        });

        // Si safe_mode está activado en la configuración, forzamos shader simple
        if initial_settings.safe_mode || crate::ui::lsd_shader::should_use_safe_mode() {
            println!("[SHADER] Safe mode enabled, using simple shader");
            lsd_shader::set_safe_mode_shader();
        }

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
                settings_state: SettingsState::new(initial_settings.clone()),
                download_progress: 0.0,
                sub_progress: 0.0,
                status_text: "Initializing...".to_string(),
                error: None,
                running_game: None,
                bg_handle: initial_bg, // Assign the handle immediately (can be Some or None)
                editing_profile: None,
                editing_uuid: None,
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
                cancellation_token: Arc::new(AtomicBool::new(false)),
                server_patch_progress: 0.0,
                show_server_patch_progress: false,
                palette: theme::generate_palette(&initial_settings.theme),
                lsd_offset: if initial_settings.theme.lsd_mode {
                    // Valores iniciales aleatorios para que el efecto LSD se aplique inmediatamente
                    let t = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs_f32();
                    let ox = (t * 1.3).sin() * 1.0 + (t * 2.8).cos() * 0.5 + (t * 0.7).sin() * 0.3;
                    let oy = (t * 0.9).cos() * 1.0 + (t * 3.5).sin() * 0.5 + (t * 1.1).cos() * 0.3;
                    (ox, oy)
                } else {
                    (0.0, 0.0)
                },
                start_time: std::time::Instant::now(),
                lsd_preview: false,
                cursor_position: iced::Point::ORIGIN,
                last_mouse_move_time: std::time::Instant::now(),
                lsd_enabled_time: if initial_settings.theme.lsd_mode {
                    Some(std::time::Instant::now())
                } else {
                    None
                },
                is_maximized: false,
                last_title_click: std::time::Instant::now(),

                // --- NUEVOS CAMPOS PARA REDIMENSIÓN ---
                resizing_direction: None,
                current_window_size: Size::new(width, height),
                current_window_pos: Point::ORIGIN,

                // Guardamos el estado AL MOMENTO DE HACER CLICK
                drag_start_window_pos: Point::ORIGIN,
                drag_start_window_size: Size::new(width, height),
                drag_start_mouse_screen_pos: Point::ORIGIN,
                // -------------------------------------

                // --- CAMPOS PARA EFECTOS TÁCTILES DEL SHADER ---
                shader_click_intensity: 0.0,
                shader_click_time: std::time::Instant::now(),
                lsd_shader_instance: std::cell::RefCell::new(None), // Se inicializará dinámicamente
                // ---------------------------------------------

                // NUEVOS CAMPOS PARA TRANSICIÓN DE SHADERS
                active_shader_idx: 0,
                next_shader_idx: 0,
                shader_transition: 0.0,
                total_shaders_available: total_shaders as u32, // Usar el valor calculado
                shader_change_timer: 0.0,
                // -------------------------------------
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
                // Esto garantiza que si pulsamos "ACTUALIZAR", la politica sea NetworkUpdate
                let policy = match trigger_status {
                    LauncherStatus::NeedsInstall | LauncherStatus::NeedsUpdate => {
                        InstallPolicy::NetworkUpdate
                    }
                    _ => {
                        // Si estaba en Ready o cualquier otro, verificamos rapido (Offline)
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
                    self.cancellation_token.clone(),
                )
            } else {
                Subscription::none()
            };

        let tray_sub = Subscription::run(tray_events);
        let menu_sub = Subscription::run(menu_events);

        let window_sub = window::events().map(|(_id, event)| Message::WindowEvent(event));

        // SUSCRIPCIÓN GLOBAL DE MOUSE
        // Siempre activa para que los cursores funcionen correctamente en todos los lados
        let global_mouse = event::listen_with(|event, _status, _id| match event {
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                Some(Message::ResizeReleased)
            }
            _ => None,
        });

        // CAMBIO AQUÍ: Eliminamos "&& self.settings.theme.lsd_mode"
        // Necesitamos rastrear el mouse siempre que la ventana sea visible para:
        // 1. Efectos LSD (si están activos)
        // 2. Redimensionamiento de ventana (siempre activo)
        // 3. Mantener self.cursor_position actualizado para el cálculo del arrastre inicial
        let mouse_sub = if self.is_window_visible {
            iced::event::listen_with(|event, _status, _window_id| {
                if let iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) = event {
                    Some(Message::CursorMoved(position))
                } else if let iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                    mouse::Button::Left,
                )) = event
                {
                    Some(Message::ShaderClicked) // Detectar clic izquierdo
                } else {
                    None
                }
            })
        } else {
            Subscription::none()
        };

        let tick_sub = if self.is_window_visible {
            if self.settings.theme.lsd_mode {
                iced::time::every(std::time::Duration::from_millis(33)).map(Message::Tick)
            } else if self.settings_state.is_open {
                iced::time::every(std::time::Duration::from_millis(66)).map(Message::Tick)
            } else {
                Subscription::none()
            }
        } else {
            Subscription::none()
        };

        // NUEVO: Escucha de teclado para cambiar shaders
        let keyboard_sub = if self.is_window_visible {
            iced::event::listen_with(|event, _status, _window_id| {
                if let Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) = event {
                    match key {
                        // Flecha derecha para siguiente shader
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowRight) => {
                            Some(Message::NextShader)
                        }
                        // Alternativa: Tecla 'S'
                        iced::keyboard::Key::Character(c) if c.as_str() == "s" => {
                            Some(Message::NextShader)
                        }
                        _ => None,
                    }
                } else {
                    None
                }
            })
        } else {
            Subscription::none()
        };

        Subscription::batch(vec![
            game_runner,
            tray_sub,
            menu_sub,
            window_sub,
            tick_sub,
            mouse_sub,
            global_mouse,
            keyboard_sub, // <--- Agregar esto al batch final
        ])
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
        match message {
            Message::CursorMoved(relative_position) => {
                self.cursor_position = relative_position;
                self.last_mouse_move_time = std::time::Instant::now();

                if let Some(dir) = self.resizing_direction {
                    // 1. Calcular dónde está el mouse en la PANTALLA ahora mismo
                    // Nota: relative_position es inestable mientras movemos la ventana,
                    // pero current_window_pos + relative_position siempre da la pos absoluta correcta.
                    let current_mouse_screen_x = self.current_window_pos.x + relative_position.x;
                    let current_mouse_screen_y = self.current_window_pos.y + relative_position.y;

                    // 2. Calcular cuánto se ha movido el mouse desde que hicimos click
                    let delta_x = current_mouse_screen_x - self.drag_start_mouse_screen_pos.x;
                    let delta_y = current_mouse_screen_y - self.drag_start_mouse_screen_pos.y;

                    // 3. Aplicar ese delta al tamaño/posición ORIGINAL (Snapshot)
                    let start_w = self.drag_start_window_size.width;
                    let start_h = self.drag_start_window_size.height;
                    let start_x = self.drag_start_window_pos.x;
                    let start_y = self.drag_start_window_pos.y;

                    let mut new_w = start_w;
                    let mut new_h = start_h;
                    let mut new_x = start_x;
                    let mut new_y = start_y;

                    let min_w = 480.0;
                    let min_h = 390.0;

                    match dir {
                        ResizeDirection::East => {
                            new_w = (start_w + delta_x).max(min_w);
                        }
                        ResizeDirection::South => {
                            new_h = (start_h + delta_y).max(min_h);
                        }
                        ResizeDirection::SouthEast => {
                            new_w = (start_w + delta_x).max(min_w);
                            new_h = (start_h + delta_y).max(min_h);
                        }
                        ResizeDirection::West => {
                            // Al mover a la izquierda, el ancho crece si delta es negativo
                            let proposed_width = start_w - delta_x;
                            if proposed_width >= min_w {
                                new_w = proposed_width;
                                new_x = start_x + delta_x;
                            } else {
                                // Si llegamos al mínimo, fijamos posición y ancho
                                new_w = min_w;
                                new_x = start_x + (start_w - min_w);
                            }
                        }
                        ResizeDirection::North => {
                            let proposed_height = start_h - delta_y;
                            if proposed_height >= min_h {
                                new_h = proposed_height;
                                new_y = start_y + delta_y;
                            } else {
                                new_h = min_h;
                                new_y = start_y + (start_h - min_h);
                            }
                        }
                        ResizeDirection::NorthWest => {
                            // North logic
                            let proposed_height = start_h - delta_y;
                            if proposed_height >= min_h {
                                new_h = proposed_height;
                                new_y = start_y + delta_y;
                            } else {
                                new_h = min_h;
                                new_y = start_y + (start_h - min_h);
                            }

                            // West logic
                            let proposed_width = start_w - delta_x;
                            if proposed_width >= min_w {
                                new_w = proposed_width;
                                new_x = start_x + delta_x;
                            } else {
                                new_w = min_w;
                                new_x = start_x + (start_w - min_w);
                            }
                        }
                        ResizeDirection::NorthEast => {
                            // North logic
                            let proposed_height = start_h - delta_y;
                            if proposed_height >= min_h {
                                new_h = proposed_height;
                                new_y = start_y + delta_y;
                            } else {
                                new_h = min_h;
                                new_y = start_y + (start_h - min_h);
                            }
                            // East logic
                            new_w = (start_w + delta_x).max(min_w);
                        }
                        ResizeDirection::SouthWest => {
                            // South logic
                            new_h = (start_h + delta_y).max(min_h);
                            // West logic
                            let proposed_width = start_w - delta_x;
                            if proposed_width >= min_w {
                                new_w = proposed_width;
                                new_x = start_x + delta_x;
                            } else {
                                new_w = min_w;
                                new_x = start_x + (start_w - min_w);
                            }
                        }
                    }

                    // 4. Aplicar cambios
                    let mut commands = Vec::new();

                    // IMPORTANTE: NO actualizar self.current_window_pos/size manualmente aquí.
                    // Deja que el sistema operativo responda con un evento WindowEvent::Moved/Resized.
                    // Esto elimina el "jitter" o locura al redimensionar hacia la izquierda/arriba.

                    if (new_x - self.current_window_pos.x).abs() > 0.5
                        || (new_y - self.current_window_pos.y).abs() > 0.5
                    {
                        commands.push(
                            window::oldest()
                                .and_then(move |id| window::move_to(id, Point::new(new_x, new_y))),
                        );
                    }

                    if (new_w - self.current_window_size.width).abs() > 0.5
                        || (new_h - self.current_window_size.height).abs() > 0.5
                    {
                        commands.push(
                            window::oldest()
                                .and_then(move |id| window::resize(id, Size::new(new_w, new_h))),
                        );
                    }

                    if !commands.is_empty() {
                        return Task::batch(commands);
                    }
                }

                return Task::none();
            }
            Message::Tick(_now) => {
                // Bloqueo de seguridad: si desactivas el LSD pero quedaba un evento en cola.
                if !self.settings.theme.lsd_mode {
                    return Task::none();
                }

                let t = self.start_time.elapsed().as_secs_f32();
                // Mezclamos multiples frecuencias para crear un movimiento caotico y organico
                // pero que se mantiene dentro de un rango razonable [-2, 2].
                let ox = (t * 1.3).sin() * 1.0 + (t * 2.8).cos() * 0.5 + (t * 0.7).sin() * 0.3;
                let oy = (t * 0.9).cos() * 1.0 + (t * 3.5).sin() * 0.5 + (t * 1.1).cos() * 0.3;
                self.lsd_offset = (ox, oy);

                // CAMBIO AUTOMÁTICO DE SHADER CADA 30 SEGUNDOS
                if self.shader_transition > 0.0 {
                    // Avanzar transición (ajustar velocidad 0.01 -> más lento, 0.05 -> rápido)
                    self.shader_transition += 0.02;

                    if self.shader_transition >= 1.0 {
                        // Transición completada
                        self.active_shader_idx = self.next_shader_idx;
                        self.shader_transition = 0.0;
                    }
                } else {
                    // Esperar tiempo para cambiar
                    self.shader_change_timer += 0.033; // ~30ms por tick
                    if self.shader_change_timer > 30.0 {
                        // Cambiar cada 30 segundos
                        self.shader_change_timer = 0.0;
                        // Siguiente shader ciclico
                        self.next_shader_idx =
                            (self.active_shader_idx + 1) % self.total_shaders_available;
                        self.shader_transition = 0.01; // Iniciar transición
                    }
                }

                return Task::none();
            }

            _ => {}
        }

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
                | Message::RequestRepairVersion(_)
                | Message::RepairFinished(_)
                | Message::OpenVersionFolder(_)
                | Message::CloseRequested
                | Message::WindowResized(_)
                | Message::AppExit
                | Message::CopyUUID(_)
                | Message::GenerateRandomUUID
                | Message::RequestMoveData(_)
                | Message::RequestUseDataLocation(_)
                | Message::StartMigrationActual(_, _)
                | Message::MigrationProgress(_)
                | Message::LauncherUpdate(_)
                | Message::CheckStatus
                | Message::BackgroundLoaded(_)
                | Message::Tick(_)
                | Message::News(_)
                | Message::LoadJavaInfo
                | Message::JavaInfoLoaded
                | Message::WindowDrag
                | Message::MinimizeWindow
                | Message::MaximizeWindow => {}
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
            Message::CursorMoved(pos) => {
                self.cursor_position = pos;
                Task::none()
            }

            Message::ShaderClicked => {
                // Disparar pulso de onda de choque en el shader
                self.shader_click_intensity = 2.0; // Pico fuerte para la onda
                self.shader_click_time = std::time::Instant::now();
                Task::none()
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
                let was_migrating = self.status == LauncherStatus::Busy;

                self.profiles = p;
                self.settings = s.clone();
                self.palette = theme::generate_palette(&self.settings.theme);

                self.settings_state.temp_settings = s;

                if self.settings.theme.lsd_mode {
                    self.lsd_enabled_time = Some(std::time::Instant::now());

                    let t = self.start_time.elapsed().as_secs_f32();
                    self.lsd_offset = ((t * 1.3).sin() * 1.0, (t * 0.9).cos() * 1.0);
                } else {
                    self.lsd_enabled_time = None;
                }

                self.localization = loc;

                let current_lang = self.settings.language.clone();
                self.localization.load_language(&current_lang);

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

                if was_migrating {
                    self.status_text = "Data moved successfully. Verifying...".to_string();
                }

                Task::batch(tasks)
            }
            Message::LoadJavaInfo => {
                let base_dir = config::get_app_dir();
                Task::perform(
                    async move {
                        match java_detection::ensure_java_available(&base_dir).await {
                            Ok(java_info) => Message::Settings(
                                SettingsMessage::JavaVersionUpdated(java_info.version),
                            ),
                            Err(e) => {
                                eprintln!("Java detection/download failed: {}", e);
                                Message::Settings(SettingsMessage::JavaInfoLoaded)
                            }
                        }
                    },
                    |msg| msg,
                )
            }
            Message::JavaInfoLoaded => {
                Task::done(Message::Settings(SettingsMessage::JavaInfoLoaded))
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
                if self.status == LauncherStatus::Playing || self.running_game.is_some() {
                    return Task::none();
                }

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

                // CAMBIO: Si recibimos un dato remoto nuevo, actualizamos cache y generamos lista
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

                self.cancellation_token.store(false, Ordering::Relaxed);

                if self.status == LauncherStatus::Playing {
                    self.running_game = None;
                    self.status = LauncherStatus::Ready;
                } else {
                    let player_name = self.profiles.get_current_profile_name();
                    let player_uuid = self.profiles.current_profile.clone();
                    let settings = self.settings.clone();
                    let target_ver = self.latest_version;

                    let trigger_status = self.status.clone();

                    self.status_text = self
                        .localization
                        .t("launcher.status.initializing")
                        .to_string();

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
                if progress >= 100.0 || speed.contains("verified") {
                    self.status = LauncherStatus::Busy;
                } else {
                    self.status = LauncherStatus::Downloading;
                }

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

                // Modificamos la logica aqui para mayor seguridad:
                // Solo salimos si es quickplay Y la ventana no esta visible.
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

                // Si veniamos de quickplay pero el usuario abrio la ventana,
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
                    let is_settings_open = self.settings_state.is_open;

                    Task::perform(
                        async move {
                            match updater::check_for_updates(&client).await {
                                Ok(Some(info)) => updater::UpdaterMessage::UpdateFound(info),
                                Ok(None) => updater::UpdaterMessage::UpdateNotFound,
                                Err(e) => updater::UpdaterMessage::Error(e.to_string()),
                            }
                        },
                        move |res| {
                            if is_settings_open {
                                match res {
                                    updater::UpdaterMessage::UpdateFound(info) => {
                                        Message::Settings(SettingsMessage::UpdateResult(Ok(Some(
                                            info,
                                        ))))
                                    }
                                    updater::UpdaterMessage::UpdateNotFound => {
                                        Message::Settings(SettingsMessage::UpdateResult(Ok(None)))
                                    }
                                    updater::UpdaterMessage::Error(e) => {
                                        Message::Settings(SettingsMessage::UpdateResult(Err(e)))
                                    }
                                    _ => Message::LauncherUpdate(res),
                                }
                            } else {
                                Message::LauncherUpdate(res)
                            }
                        },
                    )
                }
                updater::UpdaterMessage::UpdateFound(info) => {
                    // SAFETY CHECK: Do not update if game is running
                    if self.status == LauncherStatus::Playing || self.running_game.is_some() {
                        println!(
                            "Update found v{} but game is running. Skipping.",
                            info.tag_name
                        );
                        return Task::none();
                    }

                    // SAFETY CHECK 2: Do not update if Dedicated Server (separate process) is running
                    // because it locks the executable file on Windows.
                    let server_lock = SingleInstance::new("RusTaleServer_Lock").unwrap();
                    if !server_lock.is_single() {
                        println!(
                            "Update found v{} but Dedicated Server is running. Skipping.",
                            info.tag_name
                        );
                        return Task::none();
                    }

                    self.status_text = format!("Update found: v{}", info.tag_name);

                    // Utilizar la funcion helper para obtener el asset correcto segun el SO
                    if let Some(url) = updater::get_asset_url(&info) {
                        Task::done(Message::LauncherUpdate(
                            updater::UpdaterMessage::StartUpdate(url),
                        ))
                    } else {
                        eprintln!("Update found but no compatible asset for this OS!");
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
            Message::ServerPatchProgress(progress) => {
                self.server_patch_progress = progress;
                self.show_server_patch_progress = progress > 0.0 && progress < 100.0;
                Task::none()
            }
            Message::EditProfileUUID(id) => {
                self.editing_profile = None;
                if let Some(p) = self.profiles.profiles.iter().find(|p| p.id == id) {
                    self.editing_uuid = Some((p.id.clone(), p.id.clone()));
                }
                Task::none()
            }
            Message::ProfileUUIDChanged(val) => {
                if let Some((original_id, _)) = &self.editing_uuid {
                    self.editing_uuid = Some((original_id.clone(), val));
                }
                Task::none()
            }
            Message::SaveProfileUUID => {
                if let Some((original_id, new_val)) = self.editing_uuid.take() {
                    let cleaned_uuid = new_val.trim().to_string();
                    if let Ok(parsed) = uuid::Uuid::parse_str(&cleaned_uuid) {
                        let final_uuid = parsed.to_string();
                        self.profiles.update_profile_uuid(&original_id, final_uuid);
                        let profiles = self.profiles.clone();
                        Task::perform(
                            async move { config::save_profiles(&profiles).await },
                            |_| Message::None,
                        )
                    } else {
                        println!("ignored invalid UUID");
                        Task::none()
                    }
                } else {
                    Task::none()
                }
            }
            Message::CancelProfileUUIDEdit => {
                self.editing_uuid = None;
                Task::none()
            }
            Message::CopyUUID(uuid_str) => clipboard::write(uuid_str),
            Message::GenerateRandomUUID => {
                if let Some((original_id, _)) = &self.editing_uuid {
                    let new_uuid = uuid::Uuid::new_v4().to_string();
                    self.editing_uuid = Some((original_id.clone(), new_uuid));
                }
                Task::none()
            }
            Message::OpenSettings => {
                self.settings_state.open(self.settings.clone());

                self.settings_state.is_loading_versions = true;
                self.settings_state.available_versions.clear();

                let channel = self.settings.channel.clone();
                Task::done(Message::RequestVersionCheck(channel))
            }
            Message::CloseSettings => {
                self.settings_state.is_open = false;

                // Si por alguna razon el estado quedo en "Checking" (aunque con la correccion
                // anterior no deberia), esto fuerza una re-evaluacion con los settings actuales (no guardados).
                if self.status == LauncherStatus::Checking {
                    Task::done(Message::CheckStatus)
                } else {
                    Task::none()
                }
            }
            Message::SaveSettings(new_settings) => {
                let old_settings = self.settings.clone();
                self.settings = new_settings.clone();
                self.palette = crate::theme::generate_palette(&self.settings.theme);
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
            Message::Settings(SettingsMessage::PickMoveLocation) => {
                let title = "Select New Data Location";
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
                            Message::RequestMoveData(path)
                        } else {
                            Message::None
                        }
                    },
                )
            }
            Message::Settings(SettingsMessage::PickUseDataLocation) => {
                let title = self
                    .localization
                    .t("settings.select_existing_data")
                    .to_string();
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
                            Message::RequestUseDataLocation(path)
                        } else {
                            Message::None
                        }
                    },
                )
            }
            Message::Settings(SettingsMessage::OpenCurrentDataDir) => {
                util::open_game_folder();
                Task::none()
            }
            Message::Settings(SettingsMessage::WaitAndReset) => Task::perform(
                async {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                },
                |_| Message::Settings(SettingsMessage::ResetUpdateStatus),
            ),
            Message::Settings(msg) => {
                match &msg {
                    SettingsMessage::LsdToggled(val) => {
                        // GUARDIA: Solo actuar si el valor es diferente al actual
                        if self.settings.theme.lsd_mode != *val {
                            self.settings.theme.lsd_mode = *val;
                            self.settings_state.temp_settings.theme.lsd_mode = *val;

                            if *val {
                                self.lsd_enabled_time = Some(std::time::Instant::now());
                            } else {
                                self.lsd_enabled_time = None;
                                self.lsd_preview = false;
                            }
                        }
                        // Retornamos temprano para evitar procesamiento adicional
                        return Task::none();
                    }
                    SettingsMessage::LsdHovered(val) => {
                        // Solo actualizamos si cambia, para no saturar la cola de mensajes
                        if self.lsd_preview != *val {
                            self.lsd_preview = *val;
                        }
                        // No necesitamos procesar esto mas alla, retornamos temprano
                        return Task::none();
                    }
                    _ => {}
                }

                if let Some(m) = self.settings_state.update(msg) {
                    self.palette =
                        crate::theme::generate_palette(&self.settings_state.temp_settings.theme);
                    Task::done(m)
                } else {
                    self.palette =
                        crate::theme::generate_palette(&self.settings_state.temp_settings.theme);
                    Task::none()
                }
            }

            // LOGICA PARA USAR DATOS DESDE UBICACION EXISTENTE (SIN MOVER)
            Message::RequestUseDataLocation(new_path) => {
                // 1. Validaciones previas
                if self.status == LauncherStatus::Playing || self.running_game.is_some() {
                    self.error =
                        Some("Cannot change data location while game is running.".to_string());
                    return Task::none();
                }

                let current_path = config::get_app_dir();
                if new_path == current_path {
                    return Task::none();
                }

                // 2. Validar que la nueva ubicacion tenga los archivos necesarios
                if !new_path.exists() {
                    self.error = Some("Selected location does not exist.".to_string());
                    return Task::none();
                }

                // Verificar que tenga archivos basicos de RusTale
                let has_settings = new_path.join("settings.toml").exists();
                let has_profiles = new_path.join("profiles.toml").exists();

                if !has_settings && !has_profiles {
                    self.error = Some("Selected location doesn't appear to contain RusTale data (no settings.toml or profiles.toml found).".to_string());
                    return Task::none();
                }

                // 3. Cambiar solo la configuracion del launcher (SIN MOVER ARCHIVOS)
                match config::save_bootstrap_path(&new_path) {
                    Ok(_) => {
                        println!("Successfully changed data location to: {:?}", new_path);

                        self.paths = crate::game::GamePaths::new(new_path.clone());
                        self.status_text = "Data location updated successfully!".to_string();

                        // Recargar configuracion desde la nueva ubicacion
                        self.settings = config::load_settings_sync();
                        self.settings_state.temp_settings = self.settings.clone();

                        Task::none()
                    }
                    Err(e) => {
                        self.error = Some(format!("Failed to save new data location: {}", e));
                        Task::none()
                    }
                }
            }

            // LOGICA DE MIGRACION
            Message::RequestMoveData(new_path) => {
                // 1. Validaciones previas
                if self.status == LauncherStatus::Playing || self.running_game.is_some() {
                    self.error = Some("Cannot move data while game is running.".to_string());
                    return Task::none();
                }

                let current_path = config::get_app_dir();
                if new_path == current_path {
                    return Task::none();
                }

                // 2. Iniciar proceso
                self.status = LauncherStatus::Migrating;
                self.status_text = "Migrating files... (patience)".to_string();
                self.download_progress = 0.0;

                // Bloquear interaccion cerrando modales si estan abiertos
                self.settings_state.is_open = false;

                Task::perform(async move { (current_path, new_path) }, |(curr, dest)| {
                    Message::StartMigrationActual(curr, dest)
                })
            }
            Message::StartMigrationActual(curr, dest) => {
                use iced::stream;

                Task::run(
                    stream::channel(
                        100,
                        move |mut output: iced::futures::channel::mpsc::Sender<Message>| {
                            let curr_clone = curr.clone();
                            let dest_clone = dest.clone();
                            async move {
                                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
                                let tx_clone = tx.clone();

                                tokio::spawn(async move {
                                    let res = crate::util::move_dir_with_progress(
                                        curr_clone,
                                        dest_clone,
                                        move |pct| {
                                            let _ = tx_clone.send(pct);
                                        },
                                    )
                                    .await;

                                    let _ = tx.send(if res.is_ok() { 200.0 } else { -1.0 });
                                    if let Err(e) = res {
                                        eprintln!("Migration error: {}", e);
                                    }
                                });

                                loop {
                                    tokio::select! {
                                       Some(pct) = rx.recv() => {
                                           if pct == 200.0 {
                                               let _ = crate::config::save_bootstrap_path(&dest);
                                               let _ = output.send(Message::DataMoveFinished(Ok(dest.clone()))).await;
                                               break;
                                           } else if pct == -1.0 {
                                               let _ = output.send(Message::DataMoveFinished(Err("Migration error".into()))).await;
                                               break;
                                           } else {
                                               let _ = output.send(Message::MigrationProgress(pct)).await;
                                           }
                                       }
                                    }
                                }
                            }
                        },
                    ),
                    |m| m,
                )
            }
            Message::MigrationProgress(pct) => {
                self.status = LauncherStatus::Migrating;
                self.download_progress = pct;
                self.status_text = format!("Migrating files... {:.0}%", pct);
                Task::none()
            }

            Message::DataMoveFinished(result) => match result {
                Ok(new_path) => {
                    println!("Migration success to: {:?}", new_path);

                    self.paths = crate::game::GamePaths::new(new_path.clone());

                    let current_exe = std::env::current_exe().unwrap_or_default();
                    if !current_exe.starts_with(&new_path) {
                        self.status_text =
                            "Migration done! Please restart from the NEW location.".to_string();
                        crate::util::open_path(new_path.clone());
                    } else {
                        self.status_text = "Migration successful.".to_string();
                    }

                    Task::perform(
                        async {
                            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                            (config::load_profiles().await, config::load_settings().await)
                        },
                        |(p, s)| Message::ConfigLoaded(p, s, crate::lang::Localization::new()),
                    )
                }
                Err(e) => {
                    self.status = LauncherStatus::Ready;
                    self.error = Some(format!("Migration failed: {}", e));
                    Task::none()
                }
            },
            Message::DataMoveStarted => {
                self.status = LauncherStatus::Busy;
                Task::none()
            }
            Message::RequestVersionCheck(chan) => {
                let client = self.api_client.clone();
                Task::perform(
                    async move {
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
                // Actualizamos el estado del modal (lo que ve el usuario ahora)
                self.settings_state.available_versions = v.clone();

                // LOGICA IMPORTANTE:
                // Si el canal que estamos viendo en el modal (temp_settings) es igual
                // al canal guardado globalmente (settings), actualizamos la cache global.
                // Esto arregla el bug de que al volver a abrir se vean versiones viejas.
                if self.settings_state.temp_settings.channel == self.settings.channel {
                    self.available_versions = v;

                    // Si tenemos una version mas reciente detectada, actualizamos latest_version
                    if let Some(first) = self.available_versions.first() {
                        self.latest_version = Some(*first);
                    }
                }

                self.settings_state.is_loading_versions = false;

                // Actualizar lista de instalados (logica visual)
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
            Message::RequestRepairVersion(v) => {
                let mut tasks = Vec::new();

                // 1. Check if game is running
                if self.status == LauncherStatus::Playing || self.running_game.is_some() {
                    self.running_game = None;
                    self.status = LauncherStatus::Ready;
                    self.rebuild_tray_menu();

                    if !self.is_window_visible {
                        self.is_window_visible = true;
                        tasks.push(
                            window::oldest()
                                .and_then(|id: window::Id| {
                                    Task::batch(vec![
                                        window::set_mode(id, window::Mode::Windowed),
                                        window::gain_focus(id),
                                    ])
                                })
                                .then(|_: ()| Task::done(Message::None)),
                        );
                    }
                }

                self.status = LauncherStatus::Busy;
                self.status_text = "Repairing installation...".to_string();

                let base_dir = config::get_app_dir();
                let channel = self.settings.channel.clone();

                let version_str = if v == 0 {
                    "latest".to_string()
                } else {
                    v.to_string()
                };

                tasks.push(Task::perform(
                    async move {
                        crate::game::repair::repair_installation(
                            base_dir,
                            channel,
                            version_str,
                            |_, msg| {
                                println!("[Repair] {}", msg);
                            },
                        )
                        .await
                    },
                    |res| match res {
                        Ok(_) => Message::RepairFinished(Ok(())),
                        Err(e) => Message::RepairFinished(Err(e.to_string())),
                    },
                ));

                Task::batch(tasks)
            }

            Message::RepairFinished(res) => {
                match res {
                    Ok(_) => {
                        self.status = LauncherStatus::Ready;
                        self.status_text = "Repair successful.".to_string();

                        let channel = self.settings.channel.clone();

                        if self.settings_state.is_open {
                            return Task::perform(
                                async move {
                                    let base_dir = config::get_app_dir();
                                    game::install::get_installed_versions(&base_dir, &channel).await
                                },
                                Message::InstalledVersionsReceived,
                            );
                        }
                        if self.mods_state.is_open {
                            return Task::done(Message::Mods(ModsMessage::RefreshLocal));
                        }
                    }
                    Err(e) => {
                        self.status = LauncherStatus::Ready;
                        self.error = Some(format!("Repair failed: {}", e));
                    }
                }
                Task::none()
            }
            Message::DownloadError(err) => {
                if err.contains("Cancelled by user") {
                    Task::none()
                } else {
                    self.status = LauncherStatus::Ready;
                    self.error = Some(err);
                    self.running_game = None;
                    Task::none()
                }
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
            Message::Tick(_) => Task::none(),
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

                    // Igual aqui, si muestra la ventana, ya no es quickplay puro
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
                    // Si el icono existe...
                    if self.tray_icon.is_some() {
                        // [LINUX FIX] En Linux/Wayland, 'Hidden' congela la ventana.
                        // Usamos 'Minimize' que es seguro. La ventana se queda en barra de tareas
                        // pero no se congela y el icono del tray sigue funcionando.
                        #[cfg(target_os = "linux")]
                        {
                            self.is_window_visible = true; // Mantener lógica de visible para que Iced siga dibujando
                            window::oldest().and_then(|id| window::minimize(id, true))
                        }

                        // [WINDOWS] En Windows 'Hidden' funciona perfecto para Tray real.
                        #[cfg(not(target_os = "linux"))]
                        {
                            self.is_window_visible = false;
                            window::oldest()
                                .and_then(|id| window::set_mode(id, window::Mode::Hidden))
                        }
                    } else {
                        // Si no hay icono, salimos para no atrapar al usuario
                        self.save_and_exit()
                    }
                } else {
                    self.save_and_exit()
                }
            }

            Message::WindowResized(size) => {
                self.window_size = size;

                // Actualizamos los settings en memoria
                self.settings.width = size.width as u32;
                self.settings.height = size.height as u32;

                // Sincronizamos tambien el estado temporal para que, si el modal esta abierto,
                // no se sobrescriba con la resolucion vieja al pulsar "Save".
                self.settings_state.temp_settings.width = size.width as u32;
                self.settings_state.temp_settings.height = size.height as u32;

                Task::none()
            }
            Message::AppExit => self.save_and_exit(),
            Message::WindowDrag => {
                let now = std::time::Instant::now();
                let duration = now.duration_since(self.last_title_click);
                self.last_title_click = now;

                if duration < std::time::Duration::from_millis(300) {
                    // Es un doble clic -> Maximizar/Restaurar
                    self.is_maximized = !self.is_maximized;
                    return window::oldest().and_then(|id| window::toggle_maximize(id));
                } else {
                    // Es un clic simple -> Iniciar arrastre nativo
                    return window::oldest().and_then(|id| window::drag(id));
                }
            }
            Message::MinimizeWindow => {
                window::oldest().and_then(|id| {
                    // El segundo argumento es 'minimized' (true para minimizar)
                    window::minimize(id, true)
                })
            }
            Message::MaximizeWindow => {
                self.is_maximized = !self.is_maximized;
                window::oldest().and_then(|id| window::toggle_maximize(id))
            }

            // --- NUEVA LÓGICA DE REDIMENSIONAMIENTO MANUAL ---
            Message::WindowEvent(event) => {
                match event {
                    window::Event::Resized(size) => {
                        self.current_window_size = size;
                        self.window_size = size; // Sincronizar con tu variable existente
                        self.settings.width = size.width as u32;
                        self.settings.height = size.height as u32;
                        // Importante: propagar el mensaje original si lo usabas
                        return Task::done(Message::WindowResized(size));
                    }
                    window::Event::Moved(point) => {
                        self.current_window_pos = point;
                    }
                    window::Event::CloseRequested => {
                        return Task::done(Message::CloseRequested);
                    }
                    _ => {}
                }
                Task::none()
            }

            Message::ResizePressed(dir) => {
                self.resizing_direction = Some(dir);

                // Guardamos el estado inicial exacto
                self.drag_start_window_pos = self.current_window_pos;
                self.drag_start_window_size = self.current_window_size;

                // Calculamos Mouse Absoluto: Posición Ventana + Posición Mouse Relativa
                // (Usamos self.cursor_position que ya se actualiza en CursorMoved)
                self.drag_start_mouse_screen_pos = Point::new(
                    self.current_window_pos.x + self.cursor_position.x,
                    self.current_window_pos.y + self.cursor_position.y,
                );

                Task::none()
            }

            Message::ResizeReleased => {
                self.resizing_direction = None;
                Task::none()
            }

            Message::NextShader => {
                // Solo cambiar si el modo LSD está activo Y NO estamos ya en transición
                if self.settings.theme.lsd_mode && self.shader_transition <= 0.0 {
                    // 1. Calcular índice siguiente (Ciclo circular)
                    self.next_shader_idx =
                        (self.active_shader_idx + 1) % self.total_shaders_available;

                    // 2. Iniciar la transición visual
                    // Establecer en un valor pequeño pero > 0.0 arranca el fade-in en view()
                    self.shader_transition = 0.01;

                    // 3. Resetear el temporizador automático
                    // Para que no vuelva a cambiar automáticamente a los 2 segundos de que tú lo cambiaste
                    self.shader_change_timer = 0.0;

                    println!(
                        "Manual Switch: {} -> {}",
                        self.active_shader_idx, self.next_shader_idx
                    );
                }
                Task::none()
            }
            // ---------------------------------------------
            Message::CancelAction => {
                self.cancellation_token.store(true, Ordering::Relaxed);
                self.status = LauncherStatus::Ready;
                self.status_text = self.localization.t("launcher.status.ready").to_string();
                self.running_game = None;
                self.download_progress = 0.0;
                Task::none()
            }
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
            Message::LoadJavaInfo => {
                let base_dir = config::get_app_dir();
                Task::perform(
                    async move {
                        // Usar logica existente del launcher para detectar Java
                        match java_detection::ensure_java_available(&base_dir).await {
                            Ok(java_info) => Message::Settings(
                                SettingsMessage::JavaVersionUpdated(java_info.version),
                            ),
                            Err(e) => {
                                eprintln!("Java detection/download failed: {}", e);
                                Message::Settings(SettingsMessage::JavaInfoLoaded)
                            }
                        }
                    },
                    |msg| msg,
                )
            }
            Message::JavaInfoLoaded => {
                // Notificar a settings que la carga completo
                Task::done(Message::Settings(SettingsMessage::JavaInfoLoaded))
            }
            Message::Tick(_) => Task::none(),
            _ => Task::none(),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let palette = &self.palette;
        // === CALCULAR QUIETUD ===
        // Calcula cuanto tiempo (en segundos) ha pasado desde el ultimo movimiento
        let elapsed_idle = self.last_mouse_move_time.elapsed().as_secs_f32();

        // Normalizamos:
        // 0.0 seg -> 0.0 (movimiento)
        // 3.0 seg -> 1.0 (quietud total)
        let stillness = (elapsed_idle / 3.0).clamp(0.0, 1.0);

        // 1. CALCULAR PROGRESO DE TRANSICIÓN (Time Ramp)
        // ramp_alpha va de 0.0 (Invisible) a 1.0 (Totalmente Visible/Opaco)
        // Se basa únicamente en el tiempo transcurrido desde que se activó el LSD.
        let ramp_alpha = if let Some(t) = self.lsd_enabled_time {
            let elapsed = t.elapsed().as_secs_f32();
            // Transición suave basada en la constante configurada (3 segundos)
            (elapsed / theme::LSD_RAMP_UP_SECONDS).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // 2. CALCULAR INTENSIDAD DEL EFECTO VISUAL (Matemáticas del Shader)
        // Esto controla qué tan fuerte brillan los fractales o cuánto se deforma el texto.
        // Si es Preview: Intensidad máxima.
        // Si no: Respiración (1.0 -> 0.3 al estar quieto)
        let click_decay = (self.shader_click_time.elapsed().as_secs_f32() * 8.0).exp();
        let click_pulse = self.shader_click_intensity / click_decay;

        let effect_intensity = if self.lsd_preview {
            1.0 + click_pulse
        } else {
            // Formula de "respiración":
            // Cuando está quieto (stillness 1.0), baja al 30% (0.3).
            // Cuando mueve el mouse, sube al 100% (1.0).
            // NOTA: Multiplicamos por ramp_alpha aquí para que la *distorsión*
            // empiece suave, pero esto NO afecta a la transparencia de la capa.
            (ramp_alpha * (1.0 - (stillness * 0.7))) + click_pulse
        }
        .min(10.0); // Tope de seguridad

        let ctx = theme::UIContext {
            palette: *palette,
            lsd_offset: self.lsd_offset,
            lsd_enabled: self.settings.theme.lsd_mode || self.lsd_preview,
            lsd_intensity: effect_intensity, // Para los widgets usamos la calculada con respiración
            time: self.start_time.elapsed().as_secs_f32(),
            mouse_pos: self.cursor_position,
            mouse_stillness: stillness,
            is_resizing: self.resizing_direction.is_some(),
        };

        let tint_color = theme::background_tint_color(palette);

        // 2. Creamos la capa de tinte (un contenedor vacio con color de fondo)
        let tint_overlay = container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| container::Style {
                background: Some(tint_color.into()),
                ..Default::default()
            });

        let is_interaction_disabled = self.settings_state.is_open || self.mods_state.is_open;

        let left_column_content = column![
            profile_card::view(
                &self.profiles,
                &self.editing_profile,
                &self.editing_uuid,
                self.profile_dropdown_open && !is_interaction_disabled,
                &self.localization,
                ctx,
            ),
            Space::new().height(Length::Fill),
            control_section::view(
                &self.status,
                &self.settings,
                self.latest_version,
                self.download_progress,
                self.sub_progress,
                &self.status_text,
                &self.localization,
                is_interaction_disabled,
                self.server_patch_progress,
                self.show_server_patch_progress,
                ctx,
            ),
        ]
        .spacing(20);

        let show_news = self.settings.enable_news && self.window_size.width > 750.0;

        let main_content: Element<'_, Message> = if show_news {
            let left_column = theme::magic_container(
                container(left_column_content)
                    .width(Length::FillPortion(1))
                    .height(Length::Fill)
                    .padding(30)
                    .style(move |t| theme::glass_container(&palette, t))
                    .into(),
                ctx,
            );

            let right_column = theme::magic_container(
                container(
                    self.news_section
                        .view(&self.localization, is_interaction_disabled, ctx)
                        .map(Message::News),
                )
                .width(Length::FillPortion(2))
                .height(Length::Fill)
                .padding(30)
                .style(move |t| theme::container_style_transparent(&palette, t))
                .into(),
                ctx,
            );

            row![left_column, right_column]
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(20)
                .spacing(20)
                .into()
        } else {
            // MODO COMPACTO
            // Usamos Length::Fill para que ocupe todo el ancho disponible
            // Reducimos el padding para aprovechar espacio en ventanas muy pequeñas (480px)
            let padding = if self.window_size.width < 500.0 {
                10
            } else {
                30
            };

            let left_column = theme::magic_container(
                container(left_column_content)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .padding(padding) // Padding dinamico
                    .style(move |t| theme::glass_container(&palette, t))
                    .into(),
                ctx,
            );

            if self.window_size.width > 500.0 {
                row![
                    Space::new().width(Length::Fill),
                    container(left_column)
                        .width(400.0)
                        .style(move |t| theme::container_style_transparent(&palette, t)), // Max width visual
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
                    .style(move |t| theme::container_style_transparent(&palette, t))
                    .into()
            }
        };

        // --- LOGICA DE FONDO (STACK COMPUESTO) ---
        // 1. Capa Base: Imagen Estática o Color
        let base_layer: Element<'_, Message> = if let Some(handle) = &self.bg_handle {
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

        // 2. Capa Shader: Se superpone con Alpha variable
        let bg: Element<'_, Message> = if self.settings.theme.lsd_mode {
            // [FIX] Usamos ramp_alpha directamente.
            // Esto asegura que la capa del shader vaya de 0% opacidad a 100% opacidad
            // de forma lineal, independiente de si el shader "respira" o se distorsiona.
            let shader_opacity = ramp_alpha;

            // Crear o actualizar instancia del shader dinámicamente
            let mut shader_opt = self.lsd_shader_instance.borrow_mut();
            let shader_instance = if let Some(ref mut shader) = *shader_opt {
                // Actualizar posición del mouse
                shader.update_mouse_position(self.cursor_position);
                shader
            } else {
                // Crear nueva instancia
                *shader_opt = Some(lsd_shader::LsdShader::new(
                    self.start_time,
                    self.cursor_position,
                    palette.accent,
                    self.active_shader_idx,
                    shader_opacity, // <--- CORREGIDO: Controla opacidad pura (fade in)
                    effect_intensity, // <--- CORREGIDO: Controla violencia matemática del fractal
                ));
                shader_opt.as_mut().unwrap()
            };

            // Disparar trigger_click si hay un pulso activo
            if click_pulse > 0.01 {
                shader_instance.trigger_click();
            }

            let shader_layer = iced::widget::shader(shader_instance.clone())
                .width(Length::Fill)
                .height(Length::Fill);

            // Apilamos: Imagen estática abajo, Shader arriba (transicionando transparencia)
            iced::widget::stack![base_layer, shader_layer].into()
        } else {
            base_layer
        };

        // --- CONSTRUCCIÓN DE LA BARRA DE TÍTULO ---
        let title_bar = container(
            row![
                // Icono PNG
                container(
                    image(crate::util::icons::load_window_icon_handle())
                        .width(20)
                        .height(20)
                        .content_fit(ContentFit::Contain)
                )
                .padding(Padding::new(0.0).left(10.0).right(10.0)), // Padding lateral
                // Título (Arrastrable)
                mouse_area(
                    container(theme::text_small(self.title(), ctx))
                        .width(Length::Fill)
                        .align_y(Alignment::Center)
                )
                .on_press(Message::WindowDrag)
                .interaction(Interaction::Grab), // Cursor de mano al arrastrar
                // Botones de control
                row![
                    // Minimizar
                    theme::window_control_button(
                        crate::util::icons::MINUS,
                        Message::MinimizeWindow,
                        false,
                        &palette,
                        ctx
                    ),
                    // Maximizar / Restaurar
                    theme::window_control_button(
                        if self.is_maximized {
                            crate::util::icons::RESTORE
                        } else {
                            crate::util::icons::SQUARE
                        },
                        Message::MaximizeWindow,
                        false,
                        &palette,
                        ctx
                    ),
                    // Cerrar (Rojo)
                    theme::window_control_button(
                        crate::util::icons::X,
                        Message::CloseRequested,
                        true, // is_close
                        &palette,
                        ctx
                    ),
                ]
            ]
            .align_y(Alignment::Center)
            .height(32), // Altura de la barra de título
        )
        .style(move |t| theme::title_bar_style(&palette, t))
        .width(Length::Fill);

        // Estructura principal visual (Fondo + Barra + Contenido)
        let visual_content = column![
            title_bar,
            container(main_content)
                .width(Length::Fill)
                .height(Length::Fill)
        ];

        let final_view = stack![bg, tint_overlay, visual_content];

        // Contenido principal del modal (El cuadro gris con botones)
        let modal_layer = if self.settings_state.is_open {
            Some(
                container(
                    self.settings_state
                        .view(&self.localization, self.window_size, ctx)
                        .map(Message::Settings),
                )
                .style(move |t| theme::container_style_transparent(&palette, t))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(|_| container::Style {
                    background: Some(iced::Background::Color(Color::from_rgba(
                        0.0, 0.0, 0.0, 0.8,
                    ))),
                    ..Default::default()
                }),
            )
        } else if self.mods_state.is_open {
            Some(
                container(
                    self.mods_state
                        .view(&self.localization, self.window_size, ctx)
                        .map(Message::Mods),
                )
                .style(move |t| theme::container_style_transparent(&palette, t))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(|_| container::Style {
                    background: Some(iced::Background::Color(Color::from_rgba(
                        0.0, 0.0, 0.0, 0.8,
                    ))),
                    ..Default::default()
                }),
            )
        } else {
            None
        };

        // --- STACK FINAL ---
        let content_with_modal = stack![
            final_view, // Tu fondo y contenido principal (izquierda abajo)
            // 1. Capa de oscurecimiento + Modal Centrado
            if let Some(modal) = modal_layer {
                modal.into()
            } else {
                Element::from(container(Space::new()).width(Length::Fixed(0.0)))
            }
        ];

        // --- WINDOW FRAME (aplica borde redondeado y sombra) ---
        let window_frame = container(content_with_modal)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |t| theme::window_frame_style(&palette, t, self.is_maximized));

        // --- SISTEMA DE REDIMENSIONAMIENTO (8 LADOS) ---
        if self.is_maximized {
            window_frame.into()
        } else {
            let b = 10.0; // Grosor del borde para arrastrar
            let c = 20.0; // Tamaño de la zona de las esquinas

            // Helper para crear una capa de redimensión alineada
            // Envuelve la zona sensible en un contenedor de pantalla completa para posicionarla
            let handle = |dir: ResizeDirection,
                          interaction: Interaction,
                          w: Length,
                          h: Length,
                          align_x: Alignment,
                          align_y: Alignment| {
                container(
                    mouse_area(
                        container(Space::new())
                            .width(w)
                            .height(h)
                            .style(|_| container::Style::default()), // Transparente
                    )
                    .on_press(Message::ResizePressed(dir))
                    .on_release(Message::ResizeReleased)
                    .interaction(interaction),
                )
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(align_x)
                .align_y(align_y)
            };

            let resize_handles = stack![
                // 1. Bordes (Lados) - Ocupan todo el largo/ancho correspondiente
                handle(
                    ResizeDirection::North,
                    Interaction::ResizingVertically,
                    Length::Fill,
                    Length::Fixed(b),
                    Alignment::Center,
                    Alignment::Start
                ),
                handle(
                    ResizeDirection::South,
                    Interaction::ResizingVertically,
                    Length::Fill,
                    Length::Fixed(b),
                    Alignment::Center,
                    Alignment::End
                ),
                handle(
                    ResizeDirection::West,
                    Interaction::ResizingHorizontally,
                    Length::Fixed(b),
                    Length::Fill,
                    Alignment::Start,
                    Alignment::Center
                ),
                handle(
                    ResizeDirection::East,
                    Interaction::ResizingHorizontally,
                    Length::Fixed(b),
                    Length::Fill,
                    Alignment::End,
                    Alignment::Center
                ),
                // 2. Esquinas - Prioridad sobre los bordes (por estar después en el stack)
                // NW (Arriba-Izquierda) -> Cursor \
                handle(
                    ResizeDirection::NorthWest,
                    Interaction::ResizingDiagonallyDown,
                    Length::Fixed(c),
                    Length::Fixed(c),
                    Alignment::Start,
                    Alignment::Start
                ),
                // NE (Arriba-Derecha)   -> Cursor /
                handle(
                    ResizeDirection::NorthEast,
                    Interaction::ResizingDiagonallyUp,
                    Length::Fixed(c),
                    Length::Fixed(c),
                    Alignment::End,
                    Alignment::Start
                ),
                // SW (Abajo-Izquierda)  -> Cursor /
                handle(
                    ResizeDirection::SouthWest,
                    Interaction::ResizingDiagonallyUp,
                    Length::Fixed(c),
                    Length::Fixed(c),
                    Alignment::Start,
                    Alignment::End
                ),
                // SE (Abajo-Derecha)    -> Cursor \
                handle(
                    ResizeDirection::SouthEast,
                    Interaction::ResizingDiagonallyDown,
                    Length::Fixed(c),
                    Length::Fixed(c),
                    Alignment::End,
                    Alignment::End
                ),
            ];

            // STACK FINAL: resize_handles DEBE ser el último elemento para capturar eventos
            stack![
                window_frame,   // Contenido visual abajo
                resize_handles  // Capa invisible de control arriba
            ]
            .into()
        }
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

        // --- CAMBIO AQUÍ: Capturar error en lugar de .ok() ---
        let builder_result = TrayIconBuilder::new()
            .with_menu(Box::new(tray_menu))
            .with_tooltip("RusTale Launcher")
            .with_icon(icon)
            .build();

        match builder_result {
            Ok(icon) => {
                println!("Tray icon created OK.");
                // Escribir log de éxito (temporal para debug)
                Some(icon)
            }
            Err(e) => {
                // Escribir log de error
                eprintln!("ERROR: Failed to create tray icon: {}", e);
                None
            }
        }
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
