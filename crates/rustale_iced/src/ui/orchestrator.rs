use crate::config::{GameSettings, ProfilesConfig};
use crate::game::LauncherStatus;
use crate::messages::Message;
use crate::settings::SettingsState;
use crate::theme;
use crate::ui::mods_modal::ModsState;
use crate::ui::news_section::NewsSection;
use crate::ui::resources::UiResources;
use crate::ui::server_panel::ServerPanelState;
use crate::ui::visuals::VisualState;
use crate::util::MemoryStats;
use iced::{Task, mouse};
use rustale_shared::lang::Localization;
#[cfg(all(feature = "tray", windows))]
use rustale_tray;

pub struct AppViews {
    pub settings: SettingsState,
    pub mods: ModsState,
    pub news: NewsSection,
    pub server: ServerPanelState,
    pub is_news_visible: bool,
    pub update_release: Option<crate::core::updater::ReleaseInfo>,
}

impl AppViews {
    pub fn new(settings: &GameSettings) -> Self {
        Self {
            settings: SettingsState::new(settings),
            mods: ModsState::new(),
            news: NewsSection::new(),
            server: ServerPanelState::new(),
            is_news_visible: true,
            update_release: None,
        }
    }
}

pub struct UiOrchestrator {
    // State Visual
    pub views: AppViews,
    pub visuals: VisualState,
    pub resources: UiResources,

    // Cached Data (Read-Only Copy)
    pub settings: GameSettings,
    pub profiles: ProfilesConfig,
    pub status_text: String,

    // UI Feedback State
    pub displayed_status: LauncherStatus,
    pub displayed_progress: f32,
    pub displayed_step_progress: f32,
    pub displayed_eta: Option<String>,
    pub displayed_current_step: Option<usize>,
    pub displayed_total_steps: Option<usize>,
    pub error: Option<String>,

    pub localization: Localization,
    pub available_versions: Vec<u32>,
    pub latest_version: Option<u32>,
    pub installed_versions: Vec<(i32, bool)>, // (version, is_latest_folder)
    pub memory_stats: MemoryStats,
    pub cursor_position: iced::Point,

    // Internal Logic Flags
    pub is_dragging: bool,
    pub quickplay: bool,

    // BACKUP for transactional settings rollback
    pub last_known_safe_settings: Option<GameSettings>,

    // Engine → GUI channel bridge
    pub core_receiver: std::sync::Arc<
        std::sync::Mutex<Option<tokio::sync::mpsc::Receiver<crate::core::signals::FromCore>>>,
    >,
    pub window_id: Option<iced::window::Id>,
}

impl UiOrchestrator {
    pub fn new(settings: GameSettings, profiles: ProfilesConfig, visuals: VisualState) -> Self {
        Self {
            views: AppViews::new(&settings),
            visuals,
            resources: UiResources::default(),
            localization: Localization::new(),
            settings,
            profiles,
            status_text: String::new(),
            is_dragging: false,
            quickplay: false,
            last_known_safe_settings: None,
            core_receiver: std::sync::Arc::new(std::sync::Mutex::new(None)),

            // Initialization
            displayed_status: LauncherStatus::Checking,
            displayed_progress: 0.0,
            displayed_step_progress: 0.0,
            displayed_eta: None,
            displayed_current_step: None,
            displayed_total_steps: None,
            error: None,
            available_versions: Vec::new(),
            latest_version: None,
            installed_versions: Vec::new(),
            memory_stats: MemoryStats::default(),
            cursor_position: iced::Point::ORIGIN,
            window_id: None,
        }
    }

    /// Inicializa el orchestrator con toda la lógica de arranque
    /// Retorna: (Self, Task<Message>) para la inicialización asíncrona
    pub fn initialize(
        initial_settings: GameSettings,
        to_core: &tokio::sync::mpsc::Sender<crate::core::signals::ToCore>,
        core_receiver: std::sync::Arc<
            std::sync::Mutex<Option<tokio::sync::mpsc::Receiver<crate::core::signals::FromCore>>>,
        >,
        quickplay: bool,
    ) -> (Self, Task<Message>) {
        // 1. CARGA INICIAL DE SHADERS
        let total_shaders = std::panic::catch_unwind(|| {
            let shader_count = crate::ui::shader_manager::get_shader_count();
            let shader_code = crate::ui::shader_manager::build_uber_shader_with_index(0);
            crate::ui::lsd_shader::set_global_wgsl(shader_code);
            shader_count
        })
        .unwrap_or_else(|_| {
            crate::ui::lsd_shader::set_safe_mode_shader();
            1
        });

        let palette = crate::theme::generate_palette(&initial_settings.theme);
        let mut visuals = crate::ui::visuals::VisualState::new(total_shaders as u32, palette);

        // Check for safe mode
        if initial_settings.safe_mode || crate::ui::lsd_shader::should_use_safe_mode() {
            println!("[SHADER] Safe mode active (Config or System Check)");
            crate::ui::lsd_shader::set_safe_mode_shader();
        }

        // 2. Load background
        let bg_url = "https://hytale.com/static/images/backgrounds/content-upper-new-1920.jpg";
        let initial_bg_bytes =
            crate::util::image_cache::load_background_optimized_bytes_sync(bg_url);
        let bg_task = if let Some(bytes) = initial_bg_bytes {
            visuals.background_blur = Some(crate::ui::background_blur::BackgroundBlur::new(bytes));
            Task::none()
        } else {
            let bg_url_clone = bg_url.to_string();
            Task::perform(
                async move {
                    let _ = crate::util::image_cache::process_background_async_path(&bg_url_clone)
                        .await?;
                    crate::util::image_cache::load_background_optimized_bytes_sync(&bg_url_clone)
                        .ok_or_else(|| anyhow::anyhow!("Failed to reload processed background"))
                },
                |res| crate::messages::Message::BackgroundLoaded(res.map_err(|e| e.to_string())),
            )
        };

        // 3. Create orchestrator instance
        let orchestrator = Self {
            views: AppViews::new(&initial_settings),
            visuals,
            resources: UiResources::default(),
            localization: Localization::new(),
            settings: initial_settings.clone(),
            profiles: ProfilesConfig::default(),
            status_text: String::new(),
            is_dragging: false,
            quickplay,
            last_known_safe_settings: None,
            core_receiver,
            displayed_status: LauncherStatus::Checking,
            displayed_progress: 0.0,
            displayed_step_progress: 0.0,
            displayed_eta: None,
            displayed_current_step: None,
            displayed_total_steps: None,
            error: None,
            available_versions: Vec::new(),
            latest_version: None,
            installed_versions: Vec::new(),
            memory_stats: MemoryStats::default(),
            cursor_position: iced::Point::ORIGIN,
            window_id: None,
        };

        // 4. Send init signals to core
        let _ = to_core.try_send(crate::core::signals::ToCore::BootstrapSystem);

        (
            orchestrator,
            Task::batch(vec![Task::done(Message::Initialize), bg_task]),
        )
    }

    // Global Memory Trim integration
    pub fn trim_memory_ui(&mut self) {
        self.resources.global_thumbnails.clear();
        // This is much safer than clearing HashMaps scattered everywhere
    }

    /// Handle visual and UI effect events
    fn handle_visual_events(
        &mut self,
        message: &Message,
        _core_tx: &tokio::sync::mpsc::Sender<crate::core::signals::ToCore>,
    ) -> Option<Task<Message>> {
        use crate::messages::Message;

        match message {
            Message::CursorMoved(pos) => {
                self.cursor_position = *pos;
                let is_modal = self.views.settings.is_open || self.views.mods.is_open;
                self.visuals
                    .handle_cursor_moved(*pos, is_modal, &self.settings);
                None
            }
            Message::Tick(now) => {
                let is_modal = self.views.settings.is_open || self.views.mods.is_open;
                self.visuals.handle_tick(is_modal, &self.settings);
                // Actualizar tiempo actual para efectos visuales que lo necesiten
                self.visuals.current_time =
                    now.duration_since(self.visuals.start_time).as_secs_f32();
                None
            }
            Message::MousePressed => {
                self.visuals.shader_click_intensity = 1.0;
                self.visuals.shader_click_time = std::time::Instant::now();
                None
            }
            Message::ShaderClicked => {
                self.visuals.shader_click_intensity = 1.0;
                self.visuals.shader_click_time = std::time::Instant::now();
                None
            }
            Message::NextShader => {
                let now = std::time::Instant::now();
                let time_since_last_change = now.duration_since(self.visuals.shader_click_time);

                // Solo permitir cambio cada 2 segundos
                if time_since_last_change >= std::time::Duration::from_secs(2) {
                    self.visuals.next_shader_idx =
                        (self.visuals.next_shader_idx + 1) % self.visuals.total_shaders_available;
                    self.visuals.shader_click_time = now;
                    self.visuals.shader_transition = 0.0;
                }
                None
            }
            Message::NextShaderManual => {
                self.visuals.next_shader_idx =
                    (self.visuals.next_shader_idx + 1) % self.visuals.total_shaders_available;
                self.visuals.shader_click_time = std::time::Instant::now();
                self.visuals.shader_transition = 0.0;
                None
            }
            Message::MemoryStatsUpdate => {
                // Update memory statistics in UI
                None
            }
            Message::LanguageChangedInSettings(lang) => {
                // FIX: Previously the lang param was ignored and localization was reset to default.
                // Now we actually load the requested language so the UI reflects the change immediately.
                self.localization.load_language(lang);
                None
            }
            Message::InstalledVersionsReceived(versions) => {
                self.installed_versions = versions.clone();
                None
            }
            #[cfg(all(feature = "tray", windows))]
            Message::TrayMenuEvent(event) => {
                // Handle tray menu events
                match &event.id {
                    id if id == "start" => {
                        let _ = _core_tx.try_send(crate::core::signals::ToCore::LaunchGame);
                    }
                    id if id == "stop" => {
                        let _ = _core_tx.try_send(crate::core::signals::ToCore::StopGame);
                    }
                    id if id == "show_hide" => {
                        return Some(Task::done(Message::ToggleWindowVisibility));
                    }
                    id if id == "quit" => {
                        return Some(self.save_and_exit(_core_tx));
                    }
                    _ => {
                        println!("[Tray] Unknown menu item: {:?}", event.id);
                    }
                }
                None
            }
            Message::MouseReleased => {
                // Manejar release del mouse
                None
            }
            _ => None,
        }
    }

    /// Handle initialization and loading events
    fn handle_initialization_events(
        &mut self,
        message: &Message,
        to_core: &tokio::sync::mpsc::Sender<crate::core::signals::ToCore>,
    ) -> Option<Task<Message>> {
        use crate::messages::Message;

        match message {
            Message::Initialize => {
                // Shaders and background are already initialized in Self::initialize()
                // The actual data (profiles/settings) will arrive via FromCore::BootstrapCompleted
                None
            }
            Message::ConfigLoaded(p, s, loc) => {
                self.profiles = p.clone();
                self.settings = s.clone();
                self.localization = loc.clone();

                // Aplicar configuraciones visuales inmediatas
                self.visuals.palette = crate::theme::generate_palette(&self.settings.theme);

                // Disparar sincronización con Core
                let _ = to_core.try_send(crate::core::signals::ToCore::InitializeProfiles(
                    self.profiles.clone(),
                ));
                let _ = to_core.try_send(crate::core::signals::ToCore::RequestInitialStatus(
                    self.settings.clone(),
                ));

                // Iniciar carga automática de noticias
                Some(Task::batch(vec![
                    Task::done(Message::CheckStatus),
                    Task::done(Message::News(
                        crate::ui::news_section::NewsMessage::LoadNews,
                    )),
                ]))
            }
            Message::BackgroundLoaded(result) => {
                match result {
                    Ok(bytes) => {
                        self.visuals.background_blur = Some(
                            crate::ui::background_blur::BackgroundBlur::new(bytes.clone()),
                        );
                    }
                    Err(e) => {
                        eprintln!("Failed to load background: {}", e);
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Handle progress and status update events
    fn handle_progress_events(
        &mut self,
        message: &Message,
        _to_core: &tokio::sync::mpsc::Sender<crate::core::signals::ToCore>,
    ) -> Option<Task<Message>> {
        use crate::messages::Message;

        match message {
            Message::ProgressUpdate(payload) => {
                self.displayed_progress = payload.global_progress;
                self.status_text = payload.message_key.clone();
                if let Some(stats) = &payload.stats {
                    self.displayed_eta = stats.eta_str.clone();
                }
                None
            }
            Message::UpdateTotalSteps { total_steps } => {
                self.displayed_total_steps = *total_steps;
                None
            }
            _ => None,
        }
    }

    /// Handle profile management events

    /// Handle modal and window events
    fn handle_modal_events(
        &mut self,
        message: &Message,
        to_core: &tokio::sync::mpsc::Sender<crate::core::signals::ToCore>,
    ) -> Option<Task<Message>> {
        use crate::messages::Message;

        match message {
            Message::Settings(msg) => {
                use crate::settings::SettingsMessage;
                match msg {
                    SettingsMessage::LsdToggled(val) => {
                        self.visuals.lsd_preview_override = Some(*val);
                    }
                    SettingsMessage::LsdHovered(val) => {
                        // If we are hovering, show preview, otherwise revert to temp_settings (the checkbox state)
                        self.visuals.lsd_preview_override = if *val {
                            Some(true)
                        } else {
                            Some(self.views.settings.temp_settings.theme.lsd_mode)
                        };
                    }
                    _ => {}
                }
                self.views
                    .settings
                    .update(msg.clone(), &self.localization)
                    .map(Task::done)
            }
            Message::Mods(msg) => {
                // Handle mods modal messages with proper client and resources
                let client = rustale_shared::HTTP_CLIENT.clone();
                let base_dir = crate::config::get_app_dir();
                let task = self.views.mods.update(
                    msg.clone(),
                    client,
                    base_dir,
                    self.settings.clone(),
                    &mut self.resources,
                );
                Some(task.map(crate::Message::Mods))
            }
            Message::CoreEvent(evt) => self.handle_core_event(evt.clone()),
            Message::ModsLoaded(result) => {
                match result {
                    Ok(mods) => {
                        // Cargar mods en el estado del modal
                        self.views.mods.installed_mods = mods.clone();
                    }
                    Err(e) => {
                        self.views.mods.error = Some(e.clone());
                    }
                }
                None
            }
            Message::ModsLoadedComplex(result) => {
                match result {
                    Ok((mods, patches)) => {
                        // Cargar mods y patches en el estado del modal
                        self.views.mods.installed_mods = mods.clone();
                        self.views.mods.patch_mods = patches.clone();
                    }
                    Err(e) => {
                        self.views.mods.error = Some(e.clone());
                    }
                }
                None
            }
            Message::OpenMods => {
                self.views.mods.is_open = true;
                None
            }
            Message::OpenSettings => {
                self.views.settings.open(self.settings.clone());
                // Sync current known versions to the modal early
                self.views.settings.available_versions =
                    self.available_versions.iter().map(|&v| v as i32).collect();
                self.views.settings.installed_versions = self
                    .installed_versions
                    .iter()
                    .map(|&(v, l)| (v as i32, l))
                    .collect();

                let channel = self.settings.channel.clone();
                return Some(Task::done(Message::RequestVersionCheck(channel)));
            }
            Message::CloseSettings => {
                self.visuals.lsd_preview_override = None; // Revert to saved state
                self.views.settings.is_open = false;
                None
            }
            Message::SaveSettings(new_settings) => {
                self.visuals.lsd_preview_override = None; // Clear override
                self.save_settings_with_optimistic_update(new_settings.clone(), to_core)
            }
            _ => None,
        }
    }

    /// Handle news events
    fn handle_news_events(
        &mut self,
        message: &Message,
        _to_core: &tokio::sync::mpsc::Sender<crate::core::signals::ToCore>,
    ) -> Option<Task<Message>> {
        use crate::messages::Message;

        match message {
            Message::News(msg) => {
                // Manejar mensajes de news a través del news view
                Some(self.views.news.update(msg.clone(), &mut self.resources))
            }
            _ => None,
        }
    }

    /// Handle updater events
    fn handle_updater_events(
        &mut self,
        message: &Message,
        _to_core: &tokio::sync::mpsc::Sender<crate::core::signals::ToCore>,
    ) -> Option<Task<Message>> {
        use crate::messages::Message;

        match message {
            Message::LauncherUpdate(msg) => {
                use crate::core::updater::UpdaterMessage;
                match msg {
                    UpdaterMessage::CheckForUpdates => {
                        let _ = _to_core
                            .try_send(crate::core::signals::ToCore::CheckForLauncherUpdates);
                    }
                    UpdaterMessage::StartUpdate(url) => {
                        let _ = _to_core.try_send(
                            crate::core::signals::ToCore::PerformLauncherUpdate(url.clone()),
                        );
                        self.views.update_release = None;
                    }
                    UpdaterMessage::UpdateProgress(progress, message) => {
                        self.displayed_progress = *progress;
                        self.status_text = message.clone();
                    }
                    UpdaterMessage::UpdateFinished => {
                        self.status_text = "Update downloaded. Restart required.".to_string();
                    }
                    UpdaterMessage::Error(err) => {
                        self.error = Some(err.clone());
                    }
                    _ => {}
                }
                None
            }
            _ => None,
        }
    }

    /// Reconcile local server when profile changes
    fn reconcile_local_server(&mut self) {
        // TODO: Implement local server reconciliation logic
        // This would typically involve:
        // - Checking if auth server needs to be restarted with new profile
        // - Updating server configuration
        // - Restarting if necessary
    }

    /// Helper to log a message to the launcher tab of the logs panel
    fn log_to_launcher(&mut self, entry: crate::ui::server_panel::LogEntry) {
        // Use ServerMessage::LauncherLog to add the entry
        let _ = self
            .views
            .server
            .update(crate::ui::server_panel::ServerMessage::LauncherLog(entry));
    }

    /// Save settings and exit application
    fn save_and_exit(
        &self,
        core_tx: &tokio::sync::mpsc::Sender<crate::core::signals::ToCore>,
    ) -> Task<Message> {
        // Enviar señal de guardado al core para que los cambios se persistian
        let _ = core_tx.try_send(crate::core::signals::ToCore::SaveSettings(
            self.settings.clone(),
        ));

        // Notificar al core que debe cerrarse
        let _ = core_tx.try_send(crate::core::signals::ToCore::ExitApp);

        // Cerrar la ventana de Iced
        if let Some(id) = self.window_id {
            iced::window::close(id)
        } else {
            Task::none()
        }
    }

    /// Handle events from the core logic
    pub fn handle_core_event(
        &mut self,
        event: crate::core::signals::FromCore,
    ) -> Option<Task<Message>> {
        use crate::core::signals::FromCore;

        match event {
            FromCore::BootstrapCompleted { settings, profiles } => {
                self.settings = settings;
                self.profiles = profiles;
                self.views.settings = SettingsState::new(&self.settings);

                // Initialize localization
                self.localization.load_available_languages();
                self.localization.load_language(&self.settings.language);

                // Update palette based on loaded settings
                self.visuals.palette = crate::theme::generate_palette(&self.settings.theme);

                #[cfg(all(feature = "tray", windows))]
                self.rebuild_tray_menu();

                // Log to launcher panel
                self.log_to_launcher(crate::ui::server_panel::LogEntry::success(
                    "Bootstrap completed successfully.",
                ));

                return Some(Task::batch(vec![
                    Task::done(Message::News(
                        crate::ui::news_section::NewsMessage::LoadNews,
                    )),
                    Task::done(Message::CheckStatus),
                ]));
            }
            FromCore::BootstrapFailed(err) => {
                // FIX: Was setting Busy (infinite spinner). Now we set status_text so the
                // error is readable in the control section and reset to Ready so the user
                // can still interact (e.g. retry via the Play button).
                let msg = format!("Bootstrap failed: {}", err);
                self.error = Some(msg.clone());
                self.status_text = msg;
                self.displayed_status = crate::game::LauncherStatus::Ready;
                return None;
            }
            // Estado General
            FromCore::StatusChanged(status) => {
                println!("[UI] Received StatusChanged: {:?}", status);
                self.displayed_status = status.clone();
                // Actualizar estado de UI basado en el nuevo status
                match status {
                    crate::game::LauncherStatus::Ready => {
                        self.views.mods.loading = false;
                        self.views.mods.error = None;
                    }
                    crate::game::LauncherStatus::Busy => {
                        // Podríamos mostrar indicador de ocupado global
                    }
                    _ => {}
                }
            }
            FromCore::ProgressUpdate {
                phase,
                progress,
                step_progress,
                current_step,
                total_steps,
                msg_args,
                stats,
            } => {
                self.displayed_progress = progress;
                self.displayed_step_progress = step_progress;
                self.displayed_current_step = Some(current_step);
                self.displayed_total_steps = Some(total_steps);
                // FIX: Translate immediately using the stored localization and arguments.
                // This ensures the UI shows "Downloading... 45%" instead of the raw key
                // "launcher.status.downloading" and that {0}, {1} placeholders are filled.
                let args: Vec<&str> = msg_args.iter().map(|s| s.as_str()).collect();
                self.status_text = self.localization.ta(&phase, &args);
                if let Some(s) = stats {
                    self.displayed_eta = Some(s);
                }
            }
            FromCore::Error { message, fatal: _ } => {
                // FIX: Was setting Busy (leaves a permanent spinner with no way to recover).
                // Setting Ready allows the user to retry. The coordinator already sends
                // StatusChanged(Ready) after LaunchFailed, so this is safe for all error paths.
                self.error = Some(message.clone());
                self.status_text = message.clone();
                self.views.mods.error = Some(message);
                self.displayed_status = crate::game::LauncherStatus::Ready;

                // If saving failed, rollback visuals to last known safe state
                if let Some(backup) = self.last_known_safe_settings.take() {
                    println!("[UI] Save failed, rolling back settings.");
                    self.settings = backup;
                    self.visuals.palette = crate::theme::generate_palette(&self.settings.theme);
                }
            }

            FromCore::ProfilesUpdated(profiles) => {
                self.profiles = profiles;
                return None;
            }
            // Datos Específicos
            FromCore::JavaInfoLoaded(info) => {
                self.views.settings.java_version = Some(info.version);
                self.views.settings.java_info_loaded = true;
                self.views.settings.java_loading = false;
                return None;
            }
            FromCore::ModsSearchLoaded(result) | FromCore::ModSearchCompleted(result) => {
                return Some(
                    self.views
                        .mods
                        .update(
                            crate::ui::mods_modal::ModsMessage::SearchLoaded(
                                result.map_err(|e| e.to_string()),
                            ),
                            rustale_shared::network::HTTP_CLIENT.clone(),
                            crate::config::get_app_dir(),
                            self.settings.clone(),
                            &mut self.resources,
                        )
                        .map(Message::Mods),
                );
            }
            FromCore::LocalModsLoaded(result) => {
                return Some(
                    self.views
                        .mods
                        .update(
                            crate::ui::mods_modal::ModsMessage::ModsLoadedComplex(
                                result.map_err(|e| e.to_string()),
                            ),
                            rustale_shared::network::HTTP_CLIENT.clone(),
                            crate::config::get_app_dir(),
                            self.settings.clone(),
                            &mut self.resources,
                        )
                        .map(Message::Mods),
                );
            }
            FromCore::NewsLoaded(result) => {
                // FIX: Previously this arm directly mutated self.views.news.posts, which:
                //   1. Never reset loading = false → UI stuck in "Loading..." forever.
                //   2. Never dispatched image-loading tasks → thumbnails never appeared.
                //   3. Error case never set self.views.news.error → silent failure.
                // Now we delegate through NewsSection::update(NewsLoaded) which handles
                // all three concerns correctly.
                // NOTE: NewsSection::update already returns Task<Message> (not Task<NewsMessage>),
                // so no additional .map() wrapping is needed here.
                return Some(self.views.news.update(
                    crate::ui::news_section::NewsMessage::NewsLoaded(
                        result.map_err(|e| e.to_string()),
                    ),
                    &mut self.resources,
                ));
            }
            FromCore::UpdatesLoaded(result) => {
                self.views.mods.checking_updates = false;
                match result {
                    Ok((updates, cached_map)) => {
                        self.views.mods.mods_with_updates = updates.into_iter().collect();
                        self.views.mods.cached_versions = cached_map;
                        self.views.mods.update_status_cache.clear();
                    }
                    Err(error) => {
                        self.views.mods.error = Some(error.to_string());
                    }
                }
            }
            FromCore::VersionsLoaded(result) => match result {
                Ok((mod_id, versions)) => {
                    self.views.mods.loading_versions.remove(&mod_id);
                    self.views.mods.cached_versions.insert(mod_id, versions);
                }
                Err(error) => {
                    self.views.mods.error = Some(error.to_string());
                }
            },

            // Confirmaciones
            FromCore::SettingsSaved => {
                // Commit success: drop backup to confirm the transaction
                self.last_known_safe_settings = None;
                println!("[UI] Settings saved successfully, transaction committed.");
            }
            FromCore::ModOperationFinished(result) => {
                match result {
                    Ok(_) => {
                        // FIX: Trigger an actual reload so the mods list reflects
                        // the toggle/uninstall. Previously only set loading=false, leaving
                        // the UI stale. Now we dispatch a background refresh task.
                        if self.views.mods.is_open {
                            return Some(Task::done(Message::Mods(
                                crate::ui::mods_modal::ModsMessage::RefreshLocalBackground,
                            )));
                        } else {
                            self.views.mods.loading = false;
                        }
                    }
                    Err(error) => {
                        self.views.mods.error = Some(error.to_string());
                        self.views.mods.loading = false;
                    }
                }
            }
            FromCore::GameStarted => {
                self.displayed_status = crate::game::LauncherStatus::Playing;
            }
            FromCore::GameStopped => {
                self.displayed_status = crate::game::LauncherStatus::Ready;
            }
            FromCore::ReadyToDisplay => {
                self.displayed_status = crate::game::LauncherStatus::Ready;
            }
            FromCore::CacheStatsLoaded(result) => {
                match result {
                    Ok(stats) => {
                        // Cache stats loaded - could be displayed in settings or debug panel
                        println!(
                            "[Cache] Files: {}, Size: {}, Oldest: {} days, Newest: {} days",
                            stats.file_count,
                            stats.size_formatted(),
                            stats.oldest_age_days,
                            stats.newest_age_days
                        );
                    }
                    Err(e) => {
                        self.error = Some(format!("Failed to load cache stats: {}", e));
                    }
                }
            }

            // Launcher Update Events
            FromCore::LauncherUpdateCheckResult(result) => {
                // FIX: Also update the settings modal's update_btn_status so the
                // "Check for Updates" button in Settings reflects the result instead
                // of staying in "Checking..." state forever.
                match result {
                    Ok(Some(release)) => {
                        self.status_text = format!("Update available: {}", release.tag_name);
                        self.views.settings.update_btn_status =
                            crate::settings::UpdateStatus::Found(release.clone());
                        self.views.update_release = Some(release);
                    }
                    Ok(None) => {
                        self.status_text = "No updates available".to_string();
                        self.views.settings.update_btn_status =
                            crate::settings::UpdateStatus::UpToDate;
                    }
                    Err(error) => {
                        self.error = Some(format!("Update check failed: {}", error));
                        self.views.settings.update_btn_status =
                            crate::settings::UpdateStatus::Error(error.to_string());
                    }
                }
            }
            FromCore::LauncherUpdateProgress(progress, message) => {
                self.displayed_progress = progress;
                self.status_text = message;
            }
            FromCore::LauncherUpdateFinished => {
                self.status_text = self.localization.t("status.update_completed").to_string();
                self.displayed_progress = 0.0;
            }
            FromCore::MigrationFinished(result) => match result {
                Ok(new_path) => {
                    self.status_text = format!("Migration completed to: {}", new_path.display());
                }
                Err(error) => {
                    self.error = Some(format!("Migration failed: {}", error));
                }
            },
            FromCore::RepairOperationFinished(result) => match result {
                Ok(_) => {
                    self.status_text = "Repair completed successfully".to_string();
                }
                Err(error) => {
                    self.error = Some(format!("Repair failed: {}", error));
                }
            },
            FromCore::UpdateAvailable(release) => {
                if let Some(r) = release {
                    self.status_text = format!("Update available: {}", r.tag_name);
                }
            }
            FromCore::UpdateDownloadProgress(progress) => {
                self.displayed_progress = progress;
                self.status_text = format!("Downloading update... {:.1}%", progress * 100.0);
            }
            FromCore::UpdateInstalled => {
                self.status_text = "Update installed successfully".to_string();
                self.displayed_progress = 0.0;
            }
            FromCore::UpdateError(error) => {
                self.error = Some(format!("Update error: {}", error));
            }
            FromCore::VersionCacheUpdated(versions) => {
                self.available_versions = versions.into_iter().map(|v| v as u32).collect();
                if let Some(&latest) = self.available_versions.iter().max() {
                    self.latest_version = Some(latest);
                }
                // Sincronizar con el estado del modal de settings
                self.views.settings.available_versions =
                    self.available_versions.iter().map(|&v| v as i32).collect();
                self.views.settings.is_loading_versions = false;
            }
            FromCore::InstalledVersionsLoaded(versions) => {
                self.installed_versions = versions;
                // Sincronizar con el estado del modal de settings
                self.views.settings.installed_versions = self
                    .installed_versions
                    .iter()
                    .map(|&(v, l)| (v as i32, l))
                    .collect();
            }
            FromCore::ModInstallProgress(mod_id, progress) => {
                self.status_text = format!("Installing {}... {:.1}%", mod_id, progress * 100.0);
                self.displayed_progress = progress;
            }
            FromCore::ModInstallCompleted(result) => {
                match result {
                    Ok(mod_id) => {
                        self.status_text = format!("Successfully installed {}", mod_id);
                        // FIX: Dispatch a real background refresh task instead of just
                        // setting loading=true with no follow-up task, which would leave
                        // the spinner running forever.
                        if self.views.mods.is_open {
                            return Some(Task::done(Message::Mods(
                                crate::ui::mods_modal::ModsMessage::RefreshLocalBackground,
                            )));
                        } else {
                            self.views.mods.loading = false;
                        }
                    }
                    Err(error) => {
                        self.views.mods.loading = false;
                        self.error = Some(format!("Mod installation failed: {}", error));
                    }
                }
            }
            FromCore::ModUninstallCompleted(result) => {
                match result {
                    Ok(mod_id) => {
                        self.status_text = format!("Successfully uninstalled {}", mod_id);
                        // FIX: Same as ModInstallCompleted — dispatch real refresh.
                        if self.views.mods.is_open {
                            return Some(Task::done(Message::Mods(
                                crate::ui::mods_modal::ModsMessage::RefreshLocalBackground,
                            )));
                        } else {
                            self.views.mods.loading = false;
                        }
                    }
                    Err(error) => {
                        self.views.mods.loading = false;
                        self.error = Some(format!("Mod uninstallation failed: {}", error));
                    }
                }
            }
            FromCore::OperationFailed { error } => {
                self.error = Some(format!("Operation failed: {}", error));
                self.views.settings.is_loading_versions = false;
            }
        }

        Some(Task::none())
    }

    /// Calcula el alpha actual de la UI basado en el estado
    pub fn ui_alpha_actual(&self) -> f32 {
        let elapsed_idle = self.visuals.last_mouse_move_time.elapsed().as_secs_f32();
        let stillness = (elapsed_idle / 3.0).clamp(0.0, 1.0);

        if let Some(t) = self.visuals.lsd_enabled_time {
            let elapsed = t.elapsed().as_secs_f32();
            let ramp_alpha = (elapsed / theme::LSD_RAMP_UP_SECONDS).clamp(0.0, 1.0);
            0.3 + (ramp_alpha * 0.7) * (1.0 - stillness)
        } else {
            1.0
        }
    }

    /// Verifica la interacción del cursor para efectos visuales
    pub fn get_cursor_interaction(&self) -> mouse::Interaction {
        let elapsed_idle = self.visuals.last_mouse_move_time.elapsed().as_secs_f32();
        if elapsed_idle < 2.0 {
            mouse::Interaction::default()
        } else {
            mouse::Interaction::None
        }
    }

    /// Handle optimistic settings updates with rollback support
    pub fn save_settings_with_optimistic_update(
        &mut self,
        new_settings: GameSettings,
        to_core: &tokio::sync::mpsc::Sender<crate::core::signals::ToCore>,
    ) -> Option<Task<Message>> {
        // 1. Snapshot safe state before making changes
        self.last_known_safe_settings = Some(self.settings.clone());

        // 2. Optimistic Update (Visuals reflect immediately for better UX)
        self.settings = new_settings.clone();
        self.visuals.palette = crate::theme::generate_palette(&self.settings.theme);

        // 3. Command Core to persist the changes
        let _ = to_core.try_send(crate::core::signals::ToCore::SaveSettings(new_settings));

        // 4. Trigger status check after save to reflect potential channel/version changes
        Some(Task::done(Message::CheckStatus))
    }

    /// Rebuilds the tray menu based on current game state
    #[cfg(all(feature = "tray", windows))]
    pub fn rebuild_tray_menu(&mut self) {
        let is_playing = self.displayed_status == LauncherStatus::Playing;
        let icon = crate::util::icons::load_tray_icon();

        if let Err(e) = self.visuals.tray_manager.create_tray(
            is_playing,
            icon,
            &self.localization.t("tray.tooltip"),
            &self.localization,
        ) {
            eprintln!("ERROR: Failed to update tray icon: {}", e);
        }
    }

    /// View method that renders the entire UI
    pub fn view(&self, _orchestrator: &UiOrchestrator) -> iced::Element<'_, Message> {
        use iced::widget::{Space, column, container, mouse_area, row, shader, stack};
        use iced::{Color, Element, Length};

        // Live theme preview: when the settings modal is open, render every frame
        // using the in-progress (unsaved) theme so changes to accent, saturation,
        // contrast, and base-mode are reflected immediately without needing to Save.
        // Palette is Copy so this is a cheap stack allocation.
        // Live theme preview: Palette is Copy — own the value so closures can
        // capture it by move without creating a borrow of a stack-local.
        let palette = if self.views.settings.is_open {
            crate::theme::generate_palette(&self.views.settings.temp_settings.theme)
        } else {
            self.visuals.palette
        };

        let elapsed_idle = self.visuals.last_mouse_move_time.elapsed().as_secs_f32();
        let stillness = (elapsed_idle / 3.0).clamp(0.0, 1.0);

        let ramp_alpha = if let Some(t) = self.visuals.lsd_enabled_time {
            let elapsed = t.elapsed().as_secs_f32();
            (elapsed / crate::theme::LSD_RAMP_UP_SECONDS).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let lsd_active = self
            .visuals
            .lsd_preview_override
            .unwrap_or(self.settings.theme.lsd_mode);

        let click_decay = (self.visuals.shader_click_time.elapsed().as_secs_f32() * 8.0).exp();
        let click_pulse = self.visuals.shader_click_intensity / click_decay;
        let effect_intensity = if lsd_active && self.visuals.lsd_preview_override.is_some() {
            1.5 + click_pulse * 0.5
        } else {
            let base_intensity = 0.3 + (ramp_alpha * 1.2);
            let stillness_variation = (1.0 - stillness) * 0.2;
            base_intensity + stillness_variation + click_pulse * 0.5
        }
        .min(3.0);

        let is_modal_active =
            self.views.settings.is_open || self.views.mods.is_open || self.views.server.is_open;

        let ui_alpha = if is_modal_active {
            1.0
        } else {
            self.ui_alpha_actual()
        };

        let ctx = crate::theme::UIContext {
            palette: {
                let mut p = palette;
                let color_adjust = |mut c: Color| {
                    c.a *= ui_alpha;
                    c
                };
                p.accent = color_adjust(p.accent);
                p.background = color_adjust(p.background);
                p.surface = color_adjust(p.surface);
                p.text_primary = color_adjust(p.text_primary);
                p.text_secondary = color_adjust(p.text_secondary);
                p.surface_hover = color_adjust(p.surface_hover);
                p.danger = color_adjust(p.danger);
                p.success = color_adjust(p.success);
                p
            },
            lsd_offset: self.visuals.lsd_offset,
            lsd_enabled: lsd_active,
            lsd_intensity: effect_intensity,
            time: self.visuals.start_time.elapsed().as_secs_f32(),
            mouse_pos: self.cursor_position,
            mouse_stillness: stillness,
            is_resizing: false,
        };

        let tint_color = crate::theme::background_tint_color(&palette);

        let tint_overlay = container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| container::Style {
                background: Some(
                    Color {
                        a: tint_color.a * ui_alpha,
                        ..tint_color
                    }
                    .into(),
                ),
                ..Default::default()
            });

        let is_interaction_disabled = self.views.settings.is_open || self.views.mods.is_open;

        // Calculate compact mode for server panel (consistent with settings.rs threshold)
        let is_compact = self.visuals.window_size.width < 600.0;

        let left_column_content = column![
            crate::ui::profile_card::view(
                &self.profiles,
                &self.visuals.editing_profile,
                &self.visuals.editing_uuid,
                self.visuals.profile_dropdown_open && !is_interaction_disabled,
                &self.localization,
                ctx,
            ),
            Space::new().height(Length::Fill),
            crate::ui::control_section::view(
                &self.displayed_status,
                &self.settings,
                self.latest_version,
                &self.status_text,
                self.displayed_progress,
                self.displayed_step_progress,
                self.displayed_current_step,
                self.displayed_total_steps,
                self.displayed_eta.as_ref(),
                &self.localization,
                is_interaction_disabled,
                ctx,
            ),
        ]
        .spacing(20);

        let show_news = self.settings.enable_news && self.visuals.window_size.width > 750.0;

        let main_content: Element<'_, Message> = if show_news {
            let left_column = crate::theme::magic_container(
                container(left_column_content)
                    .width(Length::FillPortion(1))
                    .height(Length::Fill)
                    .padding(30)
                    .style(move |t| crate::theme::glass_container(&ctx.palette, t))
                    .into(),
                ctx,
            );

            let right_column = crate::theme::magic_container(
                container(
                    self.views
                        .news
                        .view(
                            &self.localization,
                            is_interaction_disabled,
                            &self.resources,
                            ctx,
                        )
                        .map(Message::News),
                )
                .width(Length::FillPortion(2))
                .height(Length::Fill)
                .padding(30)
                .style(move |t| crate::theme::container_style_transparent(&ctx.palette, t))
                .into(),
                ctx,
            );

            Into::<Element<'_, Message>>::into(
                container(
                    row![left_column, right_column]
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .padding(20)
                        .spacing(20),
                )
                .width(Length::Fill)
                .height(Length::Fill),
            )
        } else {
            let padding = if self.visuals.window_size.width < 500.0 {
                10
            } else {
                30
            };

            let left_column = crate::theme::magic_container(
                container(left_column_content)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .padding(padding)
                    .style(move |t| crate::theme::glass_container(&ctx.palette, t))
                    .into(),
                ctx,
            );

            if self.visuals.window_size.width > 500.0 {
                Into::<Element<'_, Message>>::into(
                    container(
                        row![
                            Space::new().width(Length::Fill),
                            container(left_column).width(400.0).style(move |t| {
                                crate::theme::container_style_transparent(&ctx.palette, t)
                            }),
                            Space::new().width(Length::Fill)
                        ]
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .padding(20),
                    )
                    .width(Length::Fill)
                    .height(Length::Fill),
                )
            } else {
                Into::<Element<'_, Message>>::into(
                    container(left_column)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .padding(10)
                        .style(move |t| crate::theme::container_style_transparent(&ctx.palette, t)),
                )
            }
        };

        let bg_layer: Element<'_, Message> = if let Some(blur) = &self.visuals.background_blur {
            shader(blur).width(Length::Fill).height(Length::Fill).into()
        } else {
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_t: &iced::Theme| crate::theme::dark_background_container(_t))
                .into()
        };

        let lsd_shader_layer = if self.settings.theme.lsd_mode {
            let mut shader_opt = self.visuals.lsd_shader_instance.borrow_mut();
            let shader_base_alpha = ramp_alpha;
            let resize_multiplier = 1.0;
            let shader_instance = if let Some(ref mut shader) = *shader_opt {
                shader.update_mouse_position(self.cursor_position);
                shader.update_shader_id(self.visuals.active_shader_idx);
                shader.update_transition(
                    self.visuals.next_shader_idx,
                    self.visuals.shader_transition,
                );
                shader.update_accent(palette.accent);
                shader.update_alpha(shader_base_alpha * resize_multiplier);
                shader.update_time(self.visuals.current_time);
                shader
            } else {
                *shader_opt = Some(crate::ui::lsd_shader::LsdShader::new(
                    self.cursor_position,
                    palette.accent,
                    effect_intensity,
                ));
                let shader = shader_opt.as_mut().unwrap();
                shader.update_alpha(shader_base_alpha * resize_multiplier);
                shader.update_time(self.visuals.current_time);
                shader
            };

            if click_pulse > 0.01 {
                shader_instance.trigger_click();
            }

            Some(
                iced::widget::shader(shader_instance.clone())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
            )
        } else {
            None
        };

        let bg: Element<'_, Message> = if let Some(lsd) = lsd_shader_layer {
            stack(vec![bg_layer, lsd]).into()
        } else {
            bg_layer
        };

        let visual_content: iced::Element<'_, Message> = container(main_content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

        let final_view = stack![bg, tint_overlay, visual_content];

        let modal_layer = if self.views.settings.is_open {
            Some(
                container(
                    self.views
                        .settings
                        .view(&self.localization, self.visuals.window_size, ctx)
                        .map(Message::Settings),
                )
                .style(move |t| crate::theme::container_style_transparent(&palette, t))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(|_| crate::theme::overlay_container(&iced::Theme::Dark)),
            )
        } else if self.views.mods.is_open {
            Some(
                container(
                    self.views
                        .mods
                        .view(
                            &self.localization,
                            self.visuals.window_size,
                            &self.resources,
                            ctx,
                        )
                        .map(Message::Mods),
                )
                .style(move |t| crate::theme::container_style_transparent(&palette, t))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(|_| crate::theme::overlay_container(&iced::Theme::Dark)),
            )
        } else if self.views.server.is_open {
            Some(
                container(crate::ui::server_panel::view(
                    &self.views.server,
                    &self.localization,
                    ctx,
                    is_compact,
                ))
                .style(move |t| crate::theme::container_style_transparent(&palette, t))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(|_| crate::theme::overlay_container(&iced::Theme::Dark)),
            )
        } else {
            None
        };

        let content_with_modal = stack![
            final_view,
            if let Some(modal) = modal_layer {
                Element::from(modal)
            } else {
                Element::from(container(Space::new()).width(Length::Fixed(0.0)))
            }
        ];

        let window_frame = container(content_with_modal)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |t| {
                crate::theme::window_frame_style(
                    &palette,
                    t,
                    self.visuals.is_maximized || self.visuals.is_fullscreen,
                )
            });

        let window_frame_area = mouse_area(window_frame)
            .interaction(self.get_cursor_interaction())
            .on_press(Message::MousePressed);

        if self.settings.theme.lsd_mode || self.visuals.lsd_preview {
            window_frame_area.on_move(Message::CursorMoved).into()
        } else {
            window_frame_area.into()
        }
    }

    /// Main update method that orchestrates all message handling
    pub fn update(
        &mut self,
        message: Message,
        core_tx: &tokio::sync::mpsc::Sender<crate::core::signals::ToCore>,
    ) -> Task<Message> {
        // 0. INTERCEPTOR LAYER: Catch ToCore signals bubbling up from sub-modules
        match &message {
            Message::Mods(crate::ui::mods_modal::ModsMessage::ToCore(signal)) => {
                let _ = core_tx.try_send(signal.clone());
                return Task::none(); // Consumed, don't pass to state update
            }
            Message::CursorMoved(pos) => {
                self.cursor_position = *pos;
            }
            // Add other interceptors here if Settings had ToCore messages directly
            _ => {}
        }

        // 1. Handle visual effects (STATE ONLY)
        self.visuals.process_message(
            &message,
            self.views.settings.is_open || self.views.mods.is_open,
            &self.settings,
        );

        // 2. Route to Logic Handlers (TASKS/IO)
        if let Some(task) = self.handle_ui_event_with_core(&message, core_tx) {
            return task;
        }

        Task::none()
    }

    /// Handle UI events with to_core channel access
    fn handle_ui_event_with_core(
        &mut self,
        message: &Message,
        core_tx: &tokio::sync::mpsc::Sender<crate::core::signals::ToCore>,
    ) -> Option<Task<Message>> {
        use crate::messages::Message;

        // Route to specialized handlers with to_core access
        match message {
            // === VISUALS (HANDLED IN UPDATE, TASKS ONLY HERE) ===
            #[cfg(all(feature = "tray", windows))]
            Message::TrayMenuEvent(_) => self.handle_visual_events(message, core_tx),

            // === EVENTOS DE INICIALIZACIÓN Y CARGA ===
            Message::Initialize | Message::ConfigLoaded(_, _, _) | Message::BackgroundLoaded(_) => {
                self.handle_initialization_events(message, core_tx)
            }

            // === EVENTOS DE JUEGO ===
            Message::StartGame
            | Message::GameLaunched(_)
            | Message::GameStopped
            | Message::CheckStatus
            | Message::DryRunFinished(_, _, _) => {
                self.handle_game_events_with_core(message, core_tx)
            }

            // === EVENTOS DE PROGRESO ===
            Message::ProgressUpdate(_) | Message::UpdateTotalSteps { .. } => {
                self.handle_progress_events(message, core_tx)
            }

            // === EVENTOS DE VERSIONES ===
            Message::RequestVersionCheck(_)
            | Message::RequestDeleteVersion(_)
            | Message::RequestRepairVersion(_)
            | Message::RepairFinished(_)
            | Message::OpenVersionFolder(_) => {
                self.handle_version_events_with_core(message, core_tx)
            }

            // === EVENTOS DE PERFILES ===
            Message::ProfileSelected(_)
            | Message::AddProfile
            | Message::EditProfile(_)
            | Message::DeleteProfile(_)
            | Message::ProfileNameChanged(_)
            | Message::SaveProfileName
            | Message::CancelProfileEdit
            | Message::EditProfileUUID(_)
            | Message::ProfileUUIDChanged(_)
            | Message::SaveProfileUUID
            | Message::CancelProfileUUIDEdit
            | Message::CopyUUID(_)
            | Message::GenerateRandomUUID
            | Message::ToggleProfileDropdown => {
                self.handle_profile_events_with_core(message, core_tx)
            }

            // === EVENTOS DE MODALS ===
            Message::Settings(_)
            | Message::Mods(_)
            | Message::CoreEvent(_)
            | Message::ModsLoaded(_)
            | Message::ModsLoadedComplex(_)
            | Message::OpenMods
            | Message::OpenSettings
            | Message::CloseSettings
            | Message::SaveSettings(_) => self.handle_modal_events(message, core_tx),

            // === EVENTOS DE SERVER PANEL ===
            Message::OpenServerPanel | Message::CloseServerPanel | Message::Server(_) => {
                self.handle_server_panel_events(message)
            }

            // === EVENTOS DE NEWS ===
            Message::News(_) => self.handle_news_events(message, core_tx),

            // === EVENTOS DE UPDATER ===
            Message::LauncherUpdate(_) => self.handle_updater_events(message, core_tx),

            // === EVENTOS DE DATOS Y MIGRACIÓN ===
            Message::RequestMoveData(_)
            | Message::RequestUseDataLocation(_)
            | Message::DataMoveStarted
            | Message::DataMoveFinished(_)
            | Message::MigrationProgress(_)
            | Message::StartMigrationActual(_, _)
            | Message::CancelAction => self.handle_migration_events_with_core(message, core_tx),

            // === EVENTOS DE JAVA ===
            Message::LoadJavaInfo | Message::JavaInfoLoaded => {
                self.handle_java_events_with_core(message, core_tx)
            }

            // === EVENTOS DE VENTANA ===
            Message::ToggleFullscreen
            | Message::WindowEvent(_, _)
            | Message::WindowResized(_)
            | Message::WindowResizedWithMaximized(_, _) => {
                self.handle_window_events_with_core(message, core_tx)
            }

            // === EVENTOS DE SISTEMA ===
            Message::AppExit | Message::ToggleWindowVisibility | Message::CloseRequested => {
                self.handle_system_events_with_core(message, core_tx)
            }

            // === CAMBIO DE IDIOMA ===
            // FIX: Previously fell to `_ => None`, so the localization was never updated
            // at runtime when the user changed language in Settings. Now properly routed
            // to handle_visual_events which calls self.localization.load_language(lang).
            Message::LanguageChangedInSettings(_) => self.handle_visual_events(message, core_tx),

            // === EVENTOS MISCELÁNEOS ===
            Message::OpenFolder | Message::RequestCacheStats => {
                self.handle_misc_events_with_core(message, core_tx)
            }

            // === EVENTOS DE WATCHDOG ===
            Message::WatchdogCheck => {
                // Request status check from core to verify game process health
                let _ = core_tx.try_send(crate::core::signals::ToCore::WatchdogCheck);
                None
            }
            Message::DownloadError(e) => {
                self.error = Some(e.clone());
                None
            }
            Message::ServerPatchProgress(p) => {
                self.displayed_progress = *p;
                None
            }
            Message::VersionsReceived(_) => None, // Dead code: real path is FromCore::VersionCacheUpdated
            _ => None,
        }
    }

    /// Handle system-level events with to_core access
    fn handle_system_events_with_core(
        &mut self,
        message: &Message,
        core_tx: &tokio::sync::mpsc::Sender<crate::core::signals::ToCore>,
    ) -> Option<Task<Message>> {
        use crate::messages::Message;

        match message {
            Message::AppExit | Message::CloseRequested => Some(self.save_and_exit(core_tx)),
            Message::ToggleWindowVisibility => {
                if let Some(id) = self.window_id {
                    self.visuals.is_visible = !self.visuals.is_visible;
                    let mode = if self.visuals.is_visible {
                        iced::window::Mode::Windowed
                    } else {
                        iced::window::Mode::Hidden
                    };
                    Some(iced::window::set_mode(id, mode))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Handle miscellaneous events that don't fit other categories
    fn handle_misc_events_with_core(
        &mut self,
        message: &Message,
        to_core: &tokio::sync::mpsc::Sender<crate::core::signals::ToCore>,
    ) -> Option<Task<Message>> {
        use crate::messages::Message;

        match message {
            Message::OpenFolder => {
                // Open the game folder in the system file manager
                let _ = to_core.try_send(crate::core::signals::ToCore::OpenGameFolder);
                None
            }
            Message::RequestCacheStats => {
                // Request cache statistics from the core
                let _ = to_core.try_send(crate::core::signals::ToCore::GetCacheStats);
                None
            }
            _ => None,
        }
    }

    /// Handle server panel events
    fn handle_server_panel_events(&mut self, message: &Message) -> Option<Task<Message>> {
        use crate::messages::Message;

        match message {
            Message::OpenServerPanel => {
                let config = rustale_server::config::ServerConfig::default();
                Some(self.views.server.open(config))
            }
            Message::CloseServerPanel => {
                self.views.server.close();
                None
            }
            Message::Server(msg) => Some(self.views.server.update(msg.clone())),
            _ => None,
        }
    }

    /// Handle game-related events with to_core access
    fn handle_game_events_with_core(
        &mut self,
        message: &Message,
        to_core: &tokio::sync::mpsc::Sender<crate::core::signals::ToCore>,
    ) -> Option<Task<Message>> {
        use crate::messages::Message;

        match message {
            Message::StartGame => {
                // FIX (STOP button bug): The play/stop button always emits Message::StartGame.
                // When the game is already running (or in a running-adjacent state), we must
                // route to ToCore::StopGame instead of ToCore::LaunchGame.
                // Previously this unconditionally sent LaunchGame, which made the engine reply
                // "Game already running" → FromCore::Error → displayed_status reset to Ready,
                // producing the contradictory "READY TO PLAY" + "Game already running" state
                // without ever killing the process.
                match self.displayed_status {
                    crate::game::LauncherStatus::Playing => {
                        let _ = to_core.try_send(crate::core::signals::ToCore::StopGame);
                        return None;
                    }
                    _ => {}
                }

                // Normal launch path
                self.error = None;
                self.status_text = String::new();
                self.displayed_progress = 0.0;
                self.displayed_eta = None;
                let _ = to_core.try_send(crate::core::signals::ToCore::LaunchGame);
                None
            }
            Message::GameLaunched(result) => {
                match result {
                    Ok(_) => {
                        self.displayed_status = crate::game::LauncherStatus::Playing;
                    }
                    Err(e) => {
                        // FIX: Was setting Busy, which leaves the spinner stuck with no way
                        // to retry. The coordinator always sends StatusChanged(Ready) on
                        // LaunchFailed via the internal channel, so this branch mirrors that
                        // behavior for any legacy callers of Message::GameLaunched.
                        self.error = Some(e.clone());
                        self.displayed_status = crate::game::LauncherStatus::Ready;
                    }
                }
                None
            }
            Message::GameStopped => {
                self.displayed_status = crate::game::LauncherStatus::Ready;
                None
            }
            Message::CheckStatus => {
                let _ = to_core.try_send(crate::core::signals::ToCore::RequestInitialStatus(
                    self.settings.clone(),
                ));
                None
            }
            Message::DryRunFinished(settings, status, exit_code) => {
                self.settings = settings.clone();
                self.displayed_status = status.clone();
                // Manejar exit_code para mostrar error si falló
                if let Some(code) = exit_code {
                    if *code != 0 {
                        self.error = Some(format!("Game exited with code: {}", code));
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Handle version management events with to_core access
    fn handle_version_events_with_core(
        &mut self,
        message: &Message,
        to_core: &tokio::sync::mpsc::Sender<crate::core::signals::ToCore>,
    ) -> Option<Task<Message>> {
        use crate::messages::Message;

        match message {
            Message::RequestVersionCheck(version) => {
                let _ = to_core.try_send(crate::core::signals::ToCore::RequestVersionCheck(
                    version.clone(),
                ));
                // Also request installed versions for the same channel
                let _ = to_core.try_send(crate::core::signals::ToCore::RequestInstalledVersions(
                    version.clone(),
                ));
                None
            }
            Message::RequestDeleteVersion(version) => {
                let _ = to_core.try_send(crate::core::signals::ToCore::RequestDeleteVersion(
                    version.clone(),
                ));
                // FIX: After deleting a version the coordinator only sends StatusChanged(Ready),
                // it never re-scans local versions. We dispatch CheckStatus which sends
                // RequestInitialStatus → scan_local_versions → InstalledVersionsLoaded so
                // the storage list in Settings refreshes automatically.
                Some(Task::done(Message::CheckStatus))
            }
            Message::RequestRepairVersion(version) => {
                let _ = to_core.try_send(crate::core::signals::ToCore::RequestRepairVersion(
                    version.clone(),
                ));
                None
            }
            Message::RepairFinished(result) => {
                match result {
                    Ok(_) => {
                        // Reparación exitosa
                    }
                    Err(e) => {
                        self.error = Some(e.clone());
                    }
                }
                None
            }
            Message::OpenVersionFolder(version) => {
                // Build the path for the specific version and open it in the file manager
                let base_dir = crate::config::get_app_dir();
                let paths = crate::game::GamePaths::new(base_dir);
                let version_str = if *version == 0 {
                    "latest".to_string()
                } else {
                    version.to_string()
                };
                let version_dir = paths.version_dir(&self.settings.channel, &version_str);
                crate::util::open_path(version_dir);
                None
            }
            _ => None,
        }
    }

    /// Handle profile management events with to_core access
    fn handle_profile_events_with_core(
        &mut self,
        message: &Message,
        to_core: &tokio::sync::mpsc::Sender<crate::core::signals::ToCore>,
    ) -> Option<Task<Message>> {
        use crate::messages::Message;

        match message {
            Message::ProfileSelected(profile) => {
                // Update current profile via Core
                let _ =
                    to_core.try_send(crate::core::signals::ToCore::SetCurrentProfile(profile.id));

                self.profiles.current_profile = profile.id;
                self.visuals.profile_dropdown_open = false;

                // Reconcile local server when profile changes
                self.reconcile_local_server();

                Some(Task::none())
            }
            Message::AddProfile => {
                self.visuals.editing_profile = Some((None, String::new()));
                Some(Task::none())
            }
            Message::EditProfile(uuid) => {
                if let Some(profile) = self.profiles.profiles.iter().find(|p| p.id == *uuid) {
                    self.visuals.editing_profile = Some((Some(*uuid), profile.name.clone()));
                }
                Some(Task::none())
            }
            Message::DeleteProfile(uuid) => {
                // Don't allow deleting last profile
                if self.profiles.profiles.len() > 1 {
                    // Delete via Core
                    let _ = to_core.try_send(crate::core::signals::ToCore::DeleteProfile(*uuid));

                    self.profiles.profiles.retain(|p| p.id != *uuid);

                    // If we deleted the current profile, switch to the first one
                    if self.profiles.current_profile == *uuid {
                        if let Some(first) = self.profiles.profiles.first() {
                            self.profiles.current_profile = first.id;
                        }
                    }

                    return Some(Task::none());
                }
                Some(Task::none())
            }
            Message::ProfileNameChanged(name) => {
                if let Some((_, ref mut current_name)) = self.visuals.editing_profile {
                    *current_name = name.clone();
                }
                Some(Task::none())
            }
            Message::SaveProfileName => {
                if let Some((id, name)) = self.visuals.editing_profile.take() {
                    if !name.trim().is_empty() {
                        match id {
                            Some(existing_id) => {
                                // Edit existing profile via Core
                                let name_clone = name.clone();
                                let _ = to_core.try_send(
                                    crate::core::signals::ToCore::UpdateProfileName(
                                        existing_id,
                                        name_clone,
                                    ),
                                );

                                // Update local state for immediate UI feedback
                                if let Some(profile) = self
                                    .profiles
                                    .profiles
                                    .iter_mut()
                                    .find(|p| p.id == existing_id)
                                {
                                    profile.name = name;
                                }
                            }
                            None => {
                                // Create new profile via Core
                                let name_clone = name.clone();
                                let _ = to_core.try_send(
                                    crate::core::signals::ToCore::CreateProfile(name_clone),
                                );
                            }
                        }
                        return Some(Task::none());
                    }
                }
                Some(Task::none())
            }
            Message::CancelProfileEdit => {
                self.visuals.editing_profile = None;
                Some(Task::none())
            }
            Message::EditProfileUUID(uuid) => {
                if let Some(profile) = self.profiles.profiles.iter().find(|p| p.id == *uuid) {
                    self.visuals.editing_uuid = Some((*uuid, profile.id.to_string()));
                }
                Some(Task::none())
            }
            Message::ProfileUUIDChanged(uuid_str) => {
                if let Some((_, ref mut current_uuid)) = self.visuals.editing_uuid {
                    *current_uuid = uuid_str.clone();
                }
                Some(Task::none())
            }
            Message::SaveProfileUUID => {
                if let Some((id, uuid_str)) = self.visuals.editing_uuid.take() {
                    if let Ok(new_uuid) = uuid::Uuid::parse_str(&uuid_str) {
                        // Update profile UUID via Core
                        let _ = to_core.try_send(crate::core::signals::ToCore::UpdateProfileUuid(
                            id, new_uuid,
                        ));

                        // Update local state for immediate UI feedback
                        if let Some(profile) =
                            self.profiles.profiles.iter_mut().find(|p| p.id == id)
                        {
                            profile.id = new_uuid;

                            // If this was the current profile, update the current_profile ID
                            if self.profiles.current_profile == id {
                                self.profiles.current_profile = new_uuid;
                            }
                        }
                        return Some(Task::none());
                    }
                }
                Some(Task::none())
            }
            Message::CancelProfileUUIDEdit => {
                self.visuals.editing_uuid = None;
                Some(Task::none())
            }
            Message::CopyUUID(uuid) => Some(iced::clipboard::write(uuid.clone())),
            Message::GenerateRandomUUID => {
                if let Some((_, ref mut uuid_str)) = self.visuals.editing_uuid {
                    *uuid_str = uuid::Uuid::new_v4().to_string();
                }
                Some(Task::none())
            }
            Message::ToggleProfileDropdown => {
                self.visuals.profile_dropdown_open = !self.visuals.profile_dropdown_open;
                Some(Task::none())
            }
            _ => None,
        }
    }

    /// Handle data migration events with to_core access
    fn handle_migration_events_with_core(
        &mut self,
        message: &Message,
        to_core: &tokio::sync::mpsc::Sender<crate::core::signals::ToCore>,
    ) -> Option<Task<Message>> {
        use crate::messages::Message;

        match message {
            Message::RequestMoveData(path) => {
                // Use StartMigrationActual instead - redirect to it
                let from = crate::config::get_app_dir();
                let to = path.clone();
                let _ = to_core.try_send(crate::core::signals::ToCore::MigrateData { from, to });
                None
            }
            Message::RequestUseDataLocation(path) => {
                // Use existing data from a different location without moving
                let _ = to_core
                    .try_send(crate::core::signals::ToCore::UseDataLocation { path: path.clone() });
                None
            }
            Message::DataMoveStarted => {
                // Indicar inicio de migración
                None
            }
            Message::DataMoveFinished(result) => {
                match result {
                    Ok(new_path) => {
                        // Actualizar configuración con nueva ruta
                        self.status_text =
                            format!("Data migration completed to: {}", new_path.display());

                        // Guardar la nueva ruta en el bootstrap config para persistencia
                        if let Err(e) = crate::config::save_bootstrap_path(new_path) {
                            eprintln!("Failed to save bootstrap path after migration: {}", e);
                            self.error = Some(format!(
                                "Migration completed but failed to save path: {}",
                                e
                            ));
                        }
                    }
                    Err(e) => {
                        self.error = Some(e.clone());
                    }
                }
                None
            }
            Message::MigrationProgress(progress) => {
                // Actualizar progreso de migración en el estado visual
                self.displayed_progress = *progress;
                self.status_text = format!("Migrating data... {:.1}%", progress * 100.0);
                None
            }
            Message::StartMigrationActual(from, to) => {
                let _ = to_core.try_send(crate::core::signals::ToCore::MigrateData {
                    from: from.clone(),
                    to: to.clone(),
                });
                None
            }
            Message::CancelAction => {
                // FIX: Forward to the engine so it calls state.cancel_all(),
                // which sets every managed task's AtomicBool cancel token to true.
                // The coordinator then resets status to Ready immediately.
                let _ = to_core.try_send(crate::core::signals::ToCore::AbortOperation);
                None
            }
            _ => None,
        }
    }

    /// Handle Java-related events with to_core access
    fn handle_java_events_with_core(
        &mut self,
        message: &Message,
        to_core: &tokio::sync::mpsc::Sender<crate::core::signals::ToCore>,
    ) -> Option<Task<Message>> {
        use crate::messages::Message;

        match message {
            Message::LoadJavaInfo => {
                let _ = to_core.try_send(crate::core::signals::ToCore::LoadJavaInfo);
                None
            }
            Message::JavaInfoLoaded => {
                // Java info cargada
                None
            }
            _ => None,
        }
    }

    /// Handle window events with to_core access
    fn handle_window_events_with_core(
        &mut self,
        message: &Message,
        core_tx: &tokio::sync::mpsc::Sender<crate::core::signals::ToCore>,
    ) -> Option<Task<Message>> {
        use crate::messages::Message;

        match message {
            Message::ToggleFullscreen => {
                if let Some(id) = self.window_id {
                    self.visuals.is_fullscreen = !self.visuals.is_fullscreen;
                    let mode = if self.visuals.is_fullscreen {
                        iced::window::Mode::Fullscreen
                    } else {
                        iced::window::Mode::Windowed
                    };
                    Some(iced::window::set_mode(id, mode))
                } else {
                    None
                }
            }
            Message::WindowEvent(id, event) => {
                // Capturar el ID de la ventana principal si aún no lo tenemos
                if self.window_id.is_none() {
                    self.window_id = Some(*id);

                    // Linux quickplay: la ventana arranca visible pero la minimizamos
                    // al taskbar de inmediato (en Windows se usa el tray icon en su lugar)
                    #[cfg(target_os = "linux")]
                    if self.quickplay {
                        return Some(iced::window::minimize(*id, true));
                    }
                }

                // Manejar eventos de ventana a nivel de UI
                match event {
                    iced::window::Event::Focused => {
                        self.visuals.is_focused = true;
                        None
                    }
                    iced::window::Event::Unfocused => {
                        self.visuals.is_focused = false;
                        None
                    }
                    iced::window::Event::Resized(new_size) => {
                        self.visuals.window_size = *new_size;
                        None
                    }
                    iced::window::Event::CloseRequested => {
                        // Cerrar la aplicación de forma limpia
                        Some(self.save_and_exit(core_tx))
                    }
                    _ => None,
                }
            }
            Message::WindowResized(size) => {
                self.visuals.window_size = *size;
                None
            }
            Message::WindowResizedWithMaximized(size, maximized) => {
                self.visuals.window_size = *size;
                self.visuals.is_maximized = *maximized;
                None
            }
            _ => None,
        }
    }

    /// Subscription method for handling system events
    pub fn subscription(&self) -> iced::Subscription<Message> {
        use iced::Subscription;

        let is_interactive = self.visuals.is_focused && !self.visuals.is_minimized;
        // Also activate cursor tracking when the LSD preview override is on (settings modal open).
        let lsd_active = self.settings.theme.lsd_mode
            || self.visuals.lsd_preview
            || self.visuals.lsd_preview_override.unwrap_or(false);

        // Base system subscriptions
        let system_sub = crate::ui::subscriptions::listen_all(
            !self.visuals.is_minimized,
            lsd_active,
            self.core_receiver.clone(), // Pass the REAL receiver
        );

        // Input & Platform subscriptions
        #[cfg(all(feature = "tray", windows))]
        let tray_sub = iced::Subscription::run(|| tray_events_internal());
        #[cfg(all(feature = "tray", windows))]
        let menu_sub = iced::Subscription::run(|| menu_events_internal());

        let mouse_press = if is_interactive {
            iced::event::listen_with(|event, _status, _window_id| match event {
                iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                    iced::mouse::Button::Left,
                )) => {
                    crate::util::register_activity();
                    Some(Message::MousePressed)
                }
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                    iced::mouse::Button::Left,
                )) => Some(Message::MouseReleased),
                _ => None,
            })
        } else {
            Subscription::none()
        };

        let mouse_cursor = if is_interactive && lsd_active {
            iced::event::listen_with(|event, _status, _window_id| {
                if let iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) = event {
                    Some(Message::CursorMoved(position))
                } else {
                    None
                }
            })
        } else {
            Subscription::none()
        };

        let keyboard_sub = if is_interactive || lsd_active {
            crate::ui::subscriptions::listen_keyboard()
        } else {
            Subscription::none()
        };

        // Server panel event subscription
        let server_sub = crate::ui::server_panel::ServerPanelState::subscription(
            self.views.server.manager_ref.clone(),
        );

        Subscription::batch(vec![
            system_sub,
            #[cfg(all(feature = "tray", windows))]
            tray_sub,
            #[cfg(all(feature = "tray", windows))]
            menu_sub,
            mouse_press,
            mouse_cursor,
            keyboard_sub,
            server_sub,
        ])
    }
}

#[cfg(all(feature = "tray", windows))]
fn tray_events_internal() -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(
        10,
        |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
            loop {
                if let Some(event) = rustale_tray::receive_tray_event() {
                    let _ = output.try_send(Message::TrayEvent(event));
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        },
    )
}

#[cfg(all(feature = "tray", windows))]
fn menu_events_internal() -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(
        10,
        |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
            loop {
                if let Some(event) = rustale_tray::receive_menu_event() {
                    let _ = output.try_send(Message::TrayMenuEvent(event));
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        },
    )
}

