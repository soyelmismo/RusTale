use iced::{Settings, Size};
use crate::messages::Message;
use crate::ui::orchestrator::UiOrchestrator;
use rustale_engine::core::signals::ToCore;
use rustale_engine::frontend::{FrontendConfig, FrontendRunner};

pub struct RusTale {
    pub orchestrator: UiOrchestrator,
    pub to_core: tokio::sync::mpsc::Sender<ToCore>,
}

impl RusTale {
    pub fn new(quickplay: bool) -> (Self, iced::Task<Message>) {
        let initial_settings = crate::config::load_settings_sync();

        // Engine → GUI channel (FromCore events)
        let (gui_tx, gui_rx) = tokio::sync::mpsc::channel(256);
        // GUI → Engine channel (ToCore commands)
        let (core_tx, core_rx) = tokio::sync::mpsc::channel(256);

        // Spawn Core in background
        tokio::spawn(async move {
            rustale_engine::core::coordinator::run(core_rx, gui_tx, None).await;
        });

        // Wrap the receiver in Arc<Mutex<Option<...>>> for the subscription bridge
        let core_receiver = std::sync::Arc::new(std::sync::Mutex::new(Some(gui_rx)));

        // Initialize UI
        let (orchestrator, init_task) = UiOrchestrator::initialize(
            initial_settings,
            &core_tx,
            core_receiver,
            quickplay,
        );

        (
            Self { orchestrator, to_core: core_tx },
            init_task
        )
    }

    pub fn title(&self) -> String {
        self.orchestrator.localization.t("launcher.title").to_string()
    }

    pub fn theme(&self) -> iced::Theme {
        iced::Theme::Dark
    }

    pub fn update(&mut self, message: Message) -> iced::Task<Message> {
        self.orchestrator.update(message, &self.to_core)
    }

    pub fn view<'a>(&'a self) -> iced::Element<'a, Message> {
        self.orchestrator.view(&self.orchestrator)
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        self.orchestrator.subscription()
    }
}

// ─── FrontendRunner impl ──────────────────────────────────────────────────────

/// Iced GUI frontend. Implements the engine's FrontendRunner trait so the
/// launcher binary can dispatch to it without knowing Iced internals.
pub struct IcedFrontend {
    pub config: FrontendConfig,
}

impl IcedFrontend {
    pub fn new(config: FrontendConfig) -> Self {
        Self { config }
    }
}

impl FrontendRunner for IcedFrontend {
    fn run(self) -> std::process::ExitCode {
        let config = self.config;
        run_ui_mode(config.quickplay, config.width, config.height)
    }
}

// ─── Internal runner ─────────────────────────────────────────────────────────

pub fn run_ui_mode(args_quickplay: bool, args_width: f32, args_height: f32) -> std::process::ExitCode {
    // Platform specific setups
    #[cfg(target_os = "linux")]
    {
        unsafe { std::env::set_var("WGPU_BACKEND", "vulkan"); }
    }

    let config_init = crate::config::load_initialization_config_sync();
    let (width_cfg, height_cfg) = crate::config::load_width_height();
    let is_quickplay = args_quickplay || config_init.quickplay;

    let width = if args_width > 0.0 { args_width } else { width_cfg };
    let height = if args_height > 0.0 { args_height } else { height_cfg };

    let settings = Settings {
        antialiasing: false,
        default_font: iced::Font::MONOSPACE,
        ..Default::default()
    };

    let window_settings = iced::window::Settings {
        size: Size::new(width, height),
        min_size: Some(Size::new(480.0, 390.0)),
        resizable: true,
        // Quickplay visibility is platform-dependent:
        //   • Windows: window hidden; tray icon is the only entry point
        //   • Linux:   window visible but immediately minimized to taskbar
        //              (no tray support on Linux)
        visible: !is_quickplay || cfg!(target_os = "linux"),
        decorations: true,
        position: iced::window::Position::Centered,
        exit_on_close_request: false,
        ..Default::default()
    };

    match iced::application(
        move || RusTale::new(is_quickplay),
        RusTale::update,
        RusTale::view,
    )
    .theme(RusTale::theme)
    .subscription(RusTale::subscription)
    .title(RusTale::title)
    .window(window_settings)
    .settings(settings)
    .scale_factor(|app: &RusTale| app.orchestrator.settings.scale_factor)
    .run()
    {
        Ok(_) => std::process::ExitCode::SUCCESS,
        Err(_) => std::process::ExitCode::FAILURE,
    }
}
