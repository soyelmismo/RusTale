/// Multi-source log panel with tabs.
///
/// Provides a unified view of logs from different sources:
/// - Launcher: Core engine events (FromCore channel)
/// - Game Client: stdout/stderr from the game process (future)
/// - Auth Server: Auth server logs (via broadcast)
/// - Game Server: Dedicated server logs (ServerManager)
///
/// Drop `LogsPanelState` into any `AppViews`-like struct, call `view()` to
/// render it and route `Message::Server(msg)` through `update()`.
use std::collections::HashMap;
use std::collections::VecDeque;
use std::hash::Hash;
use std::sync::{Arc, Mutex};

use iced::{
    widget::{button, column, container, row, scrollable, text, text_input, mouse_area, Space},
    Color, Element, Length, Task,
};
use iced::advanced::subscription::{self, Recipe};

use crate::messages::Message;
use crate::theme::UIContext;
use rustale_server::{ServerEvent, ServerManager, ServerState};

// ─── Constants ───────────────────────────────────────────────────────────────

/// Maximum number of log lines kept in memory per tab.
const MAX_LOG_LINES: usize = 1000;


// ─── Log Tab ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LogTab {
    #[default]
    Launcher,
    GameClient,
    AuthServer,
    GameServer,
}

impl LogTab {
    pub fn label_key(&self) -> &'static str {
        match self {
            LogTab::Launcher => "logs.launcher",
            LogTab::GameClient => "logs.game_client",
            LogTab::AuthServer => "logs.auth_server",
            LogTab::GameServer => "logs.game_server",
        }
    }

    pub fn all() -> &'static [LogTab] {
        &[LogTab::Launcher, LogTab::GameClient, LogTab::AuthServer, LogTab::GameServer]
    }
}

// ─── Log entry ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum LogKind {
    Info,
    Warning,
    Error,
    Success,
    /// Custom ANSI color (R, G, B)
    Custom(f32, f32, f32),
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub kind: LogKind,
    pub text: String,
    pub timestamp: Option<String>,
    /// Original text with ANSI codes stripped
    pub raw_text: String,
}

impl LogEntry {
    pub fn info(text: impl Into<String>) -> Self {
        let t = text.into();
        Self { kind: LogKind::Info, raw_text: t.clone(), text: t, timestamp: None }
    }
    
    pub fn warning(text: impl Into<String>) -> Self {
        let t = text.into();
        Self { kind: LogKind::Warning, raw_text: t.clone(), text: t, timestamp: None }
    }
    
    pub fn error(text: impl Into<String>) -> Self {
        let t = text.into();
        Self { kind: LogKind::Error, raw_text: t.clone(), text: t, timestamp: None }
    }
    
    pub fn success(text: impl Into<String>) -> Self {
        let t = text.into();
        Self { kind: LogKind::Success, raw_text: t.clone(), text: t, timestamp: None }
    }
    
    /// Create entry with parsed ANSI colors
    pub fn with_ansi(text: impl Into<String>) -> Self {
        let t = text.into();
        let (stripped, color) = parse_ansi_colors(&t);
        Self { 
            kind: color.unwrap_or(LogKind::Info), 
            raw_text: stripped.clone(),
            text: stripped, 
            timestamp: None 
        }
    }
    
    pub fn with_timestamp(mut self) -> Self {
        let now = chrono::Local::now();
        self.timestamp = Some(now.format("%H:%M:%S").to_string());
        self
    }
}

/// Parse ANSI color codes from text, returns (stripped_text, optional_color)
/// Based on Java server log colors:
/// - 91: Red (ERROR/SEVERE)
/// - 93: Yellow (WARNING/WARN)  
/// - 92: Green (INFO)
/// - 96: Cyan (DEBUG/FINE/FINER/FINEST)
/// - 95: Magenta (CONFIG)
/// - 97: White (default)
fn parse_ansi_colors(text: &str) -> (String, Option<LogKind>) {
    let mut result = String::with_capacity(text.len());
    let mut current_color: Option<LogKind> = None;
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    
    while i < chars.len() {
        if chars[i] == '\x1b' && i + 1 < chars.len() && chars[i + 1] == '[' {
            // Found ESC[, look for 'm' terminator
            let mut j = i + 2;
            let mut code_str = String::new();
            
            while j < chars.len() && chars[j] != 'm' {
                if chars[j].is_ascii_digit() || chars[j] == ';' {
                    code_str.push(chars[j]);
                }
                j += 1;
            }
            
            if j < chars.len() && chars[j] == 'm' {
                // Parse the color code
                let codes: Vec<u8> = code_str.split(';')
                    .filter_map(|s| s.parse().ok())
                    .collect();
                
                if codes.contains(&0) {
                    // Reset
                    current_color = None;
                } else if codes.contains(&91) {
                    // Red - ERROR/SEVERE
                    current_color = Some(LogKind::Error);
                } else if codes.contains(&93) {
                    // Yellow - WARNING/WARN
                    current_color = Some(LogKind::Warning);
                } else if codes.contains(&92) {
                    // Green - INFO
                    current_color = Some(LogKind::Success);
                } else if codes.contains(&96) {
                    // Cyan - DEBUG/FINE
                    current_color = Some(LogKind::Custom(0.4, 0.9, 0.9));
                } else if codes.contains(&95) {
                    // Magenta - CONFIG
                    current_color = Some(LogKind::Custom(1.0, 0.4, 1.0));
                } else if codes.contains(&97) {
                    // White - default
                    current_color = None;
                }
                
                i = j + 1;
                continue;
            }
        }
        
        result.push(chars[i]);
        i += 1;
    }
    
    (result, current_color)
}

// ─── Messages ────────────────────────────────────────────────────────────────

/// Messages produced by or consumed by the logs panel.
/// Wrap with `Message::Server(...)` to route through the orchestrator.
#[derive(Debug, Clone)]
pub enum ServerMessage {
    // ── Tab Navigation ─────────────────────────────────────────────────────
    SelectTab(LogTab),
    
    // ── Game Server Controls ───────────────────────────────────────────────
    Start,
    Stop,
    Restart,
    CommandInput(String),
    SendCommand,

    // ── Events from various sources ────────────────────────────────────────
    /// Event from the Game Server manager
    Event(ServerEventWrapper),
    /// Log entry from the Launcher (core events)
    LauncherLog(LogEntry),
    /// Log entry from the Game Client
    GameClientLog(LogEntry),
    /// Log entry from the Auth Server
    AuthServerLog(LogEntry),
    /// Clear logs for the current tab
    ClearCurrentTab,
}

/// Newtype wrapper so `ServerEvent` can derive `Clone + Debug`.
#[derive(Debug, Clone)]
pub struct ServerEventWrapper(pub ServerEvent);

// ─── State ───────────────────────────────────────────────────────────────────

pub struct ServerPanelState {
    /// Whether the panel is currently shown.
    pub is_open: bool,

    /// Currently active tab.
    pub active_tab: LogTab,

    /// Log buffers per tab.
    pub logs: HashMap<LogTab, VecDeque<LogEntry>>,

    // ── Game Server State ──────────────────────────────────────────────────
    /// The managed game server handle.
    pub manager: Option<ServerManager>,

    /// Current lifecycle state (mirrors the last `StateChanged` event).
    pub server_state: ServerState,

    /// Text currently typed in the command input box.
    pub command_input: String,

    /// Shared reference to the ServerManager for subscriptions to call .subscribe().
    pub manager_ref: Arc<Mutex<Option<ServerManager>>>,
}

impl Default for ServerPanelState {
    fn default() -> Self {
        let mut logs = HashMap::new();
        for tab in LogTab::all() {
            logs.insert(*tab, VecDeque::with_capacity(MAX_LOG_LINES));
        }
        
        Self {
            is_open: false,
            active_tab: LogTab::default(),
            logs,
            manager: None,
            server_state: ServerState::Idle,
            command_input: String::new(),
            manager_ref: Arc::new(Mutex::new(None)),
        }
    }
}

impl ServerPanelState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ensure a `ServerManager` exists (lazy init) and open the panel.
    pub fn open(&mut self, config: rustale_server::config::ServerConfig) -> Task<Message> {
        self.is_open = true;

        if self.manager.is_none() {
            let mgr = ServerManager::new(config);
            self.manager = Some(mgr.clone());
            *self.manager_ref.lock().unwrap() = Some(mgr);
            self.push_log(LogTab::GameServer, LogEntry::info("[Panel] Server manager initialized."));
        }

        // Add welcome message to launcher tab
        if self.logs.get(&LogTab::Launcher).map_or(true, |l| l.is_empty()) {
            self.push_log(LogTab::Launcher, LogEntry::info("[Panel] Launcher events will appear here."));
        }

        Task::none()
    }

    /// Close the panel (does NOT stop any server).
    pub fn close(&mut self) {
        self.is_open = false;
    }

    fn push_log(&mut self, tab: LogTab, entry: LogEntry) {
        if let Some(buffer) = self.logs.get_mut(&tab) {
            if buffer.len() >= MAX_LOG_LINES {
                buffer.pop_front();
            }
            buffer.push_back(entry);
        }
    }

    fn clear_current_tab(&mut self) {
        if let Some(buffer) = self.logs.get_mut(&self.active_tab) {
            buffer.clear();
        }
    }

    // ── Update ───────────────────────────────────────────────────────────────

    pub fn update(&mut self, msg: ServerMessage) -> Task<Message> {
        match msg {
            ServerMessage::SelectTab(tab) => {
                self.active_tab = tab;
            }
            ServerMessage::ClearCurrentTab => {
                self.clear_current_tab();
            }
            ServerMessage::Start => {
                if let Some(mgr) = &self.manager {
                    let mgr = mgr.clone();
                    self.push_log(LogTab::GameServer, LogEntry::info("[Panel] Sending Start..."));
                    return Task::perform(
                        async move { mgr.start().await },
                        |res| {
                            if let Err(e) = res {
                                Message::Server(ServerMessage::Event(ServerEventWrapper(
                                    ServerEvent::ErrLine(format!("[Panel] Start failed: {}", e)),
                                )))
                            } else {
                                Message::None
                            }
                        },
                    );
                }
            }
            ServerMessage::Stop => {
                if let Some(mgr) = &self.manager {
                    let mgr = mgr.clone();
                    self.push_log(LogTab::GameServer, LogEntry::info("[Panel] Sending Stop..."));
                    return Task::perform(
                        async move { mgr.stop().await },
                        |res| {
                            if let Err(e) = res {
                                Message::Server(ServerMessage::Event(ServerEventWrapper(
                                    ServerEvent::ErrLine(format!("[Panel] Stop failed: {}", e)),
                                )))
                            } else {
                                Message::None
                            }
                        },
                    );
                }
            }
            ServerMessage::Restart => {
                if let Some(mgr) = &self.manager {
                    let mgr = mgr.clone();
                    self.push_log(LogTab::GameServer, LogEntry::info("[Panel] Sending Restart..."));
                    return Task::perform(
                        async move { mgr.restart().await },
                        |res| {
                            if let Err(e) = res {
                                Message::Server(ServerMessage::Event(ServerEventWrapper(
                                    ServerEvent::ErrLine(format!("[Panel] Restart failed: {}", e)),
                                )))
                            } else {
                                Message::None
                            }
                        },
                    );
                }
            }
            ServerMessage::CommandInput(s) => {
                self.command_input = s;
            }
            ServerMessage::SendCommand => {
                let cmd = self.command_input.trim().to_string();
                if cmd.is_empty() {
                    return Task::none();
                }
                self.push_log(LogTab::GameServer, LogEntry::info(format!("> {}", cmd)));
                self.command_input.clear();

                if let Some(mgr) = &self.manager {
                    let mgr = mgr.clone();
                    return Task::perform(
                        async move { mgr.send_command(cmd).await },
                        |res| {
                            if let Err(e) = res {
                                Message::Server(ServerMessage::Event(ServerEventWrapper(
                                    ServerEvent::ErrLine(format!("[Panel] Command failed: {}", e)),
                                )))
                            } else {
                                Message::None
                            }
                        },
                    );
                }
            }
            ServerMessage::Event(ServerEventWrapper(evt)) => {
                match &evt {
                    ServerEvent::StateChanged(state) => {
                        self.server_state = state.clone();
                        self.push_log(LogTab::GameServer, LogEntry::info(format!("[State] → {}", state)));
                    }
                    ServerEvent::LogLine(line) => {
                        self.push_log(LogTab::GameServer, LogEntry::info(line.clone()));
                    }
                    ServerEvent::ErrLine(line) => {
                        self.push_log(LogTab::GameServer, LogEntry::error(line.clone()));
                    }
                }
            }
            ServerMessage::LauncherLog(entry) => {
                self.push_log(LogTab::Launcher, entry);
            }
            ServerMessage::GameClientLog(entry) => {
                self.push_log(LogTab::GameClient, entry);
            }
            ServerMessage::AuthServerLog(entry) => {
                self.push_log(LogTab::AuthServer, entry);
            }
        }

        Task::none()
    }

    // ── Iced subscriptions ────────────────────────────────────────────────────

    /// Returns the iced `Subscription` that pumps events into the message loop.
    /// Pass the manager_ref so subscriptions can call .subscribe() to get their own receiver.
    pub fn subscription(
        manager_ref: Arc<Mutex<Option<ServerManager>>>,
    ) -> iced::Subscription<Message> {
        let game_server_sub = subscription::from_recipe(GameServerEventSubscription {
            manager_ref: manager_ref.clone(),
        });
        
        iced::Subscription::batch(vec![game_server_sub])
    }
}

// ─── Subscription: Game Server Events ─────────────────────────────────────────

struct GameServerEventSubscription {
    manager_ref: Arc<Mutex<Option<ServerManager>>>,
}

impl Recipe for GameServerEventSubscription {
    type Output = Message;

    fn hash(&self, state: &mut iced::advanced::subscription::Hasher) {
        std::any::TypeId::of::<Self>().hash(state);
    }

    fn stream(
        self: Box<Self>,
        _input: subscription::EventStream,
    ) -> iced::futures::stream::BoxStream<'static, Self::Output> {
        let manager_ref = self.manager_ref;
        Box::pin(iced::stream::channel(256, move |mut output: iced::futures::channel::mpsc::Sender<Message>| {
            let manager_ref = manager_ref.clone();
            async move {
                // Wait for the manager to be available (poll every 100ms)
                let mut rx = loop {
                    let maybe_rx = {
                        let guard = manager_ref.lock().unwrap();
                        guard.as_ref().map(|mgr| mgr.subscribe())
                    };
                    if let Some(r) = maybe_rx {
                        break r;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                };

                loop {
                    match rx.recv().await {
                        Ok(evt) => {
                            let msg = Message::Server(ServerMessage::Event(ServerEventWrapper(evt)));
                            if output.try_send(msg).is_err() {
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            let warn = Message::Server(ServerMessage::Event(ServerEventWrapper(
                                ServerEvent::ErrLine(format!(
                                    "[Panel] Warning: dropped {} log lines (subscriber too slow)",
                                    n
                                )),
                            )));
                            let _ = output.try_send(warn);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }))
    }
}

// ─── View ────────────────────────────────────────────────────────────────────

/// Render the logs panel with responsive design.
pub fn view<'a>(
    state: &'a ServerPanelState,
    localization: &'a rustale_shared::lang::Localization,
    ctx: UIContext,
    is_compact: bool,
) -> Element<'a, Message> {
    let palette = ctx.palette;
    let padding = if is_compact { 10 } else { 20 };

    // ── Tab Bar (scrollable in compact mode) ───────────────────────────────────
    let tab_buttons: Element<'a, Message> = LogTab::all()
        .iter()
        .fold(row![].spacing(4), |row, tab| {
            let is_active = state.active_tab == *tab;
            let (label, count) = get_tab_label_with_count(state, tab, localization, ctx);
            
            let label_text = crate::theme::text(
                text(label).size(if is_compact { 11 } else { 12 }),
                ctx,
            );
            
            let count_text = if count > 0 && !is_compact {
                crate::theme::text_muted(format!("({})", count.min(999)), ctx)
            } else {
                crate::theme::text_muted("", ctx)
            };
            
            let btn = button(
                row![label_text, count_text]
                    .spacing(4)
                    .align_y(iced::Alignment::Center),
            )
            .padding(if is_compact { [4, 8] } else { [6, 12] })
            .style(move |t, s| {
                if is_active {
                    crate::theme::active_tab_style(&palette, t, s)
                } else {
                    crate::theme::secondary_button_style(&palette, t, s)
                }
            })
            .on_press(Message::Server(ServerMessage::SelectTab(*tab)));
            
            row.push(btn)
        })
        .into();

    // ── Tab-specific controls ────────────────────────────────────────────────
    let controls = view_tab_controls(state, localization, ctx, is_compact);

    // ── Log area with custom colors support ─────────────────────────────────────
    let log_content = state
        .logs
        .get(&state.active_tab)
        .map(|buffer| {
            buffer.iter().fold(column![].spacing(1), |col, entry| {
                // Use palette colors for log kinds, support Custom ANSI colors
                let color = match &entry.kind {
                    LogKind::Error => palette.danger,
                    LogKind::Warning => Color::from_rgb(1.0, 0.85, 0.3),
                    LogKind::Success => palette.success,
                    LogKind::Info => palette.text_primary,
                    LogKind::Custom(r, g, b) => Color::from_rgb(*r, *g, *b),
                };
                
                let timestamp_part = entry.timestamp.as_ref().map(|t| {
                    crate::theme::text_micro(format!("[{}] ", t), ctx)
                });
                
                // Wrap text for small screens
                let msg_text = crate::theme::text(
                    text(&entry.text)
                        .size(if is_compact { 10 } else { 11 })
                        .color(color)
                        .font(iced::Font::MONOSPACE)
                        .shaping(iced::widget::text::Shaping::Advanced),
                    ctx,
                );
                
                let line: Element<'a, Message> = match timestamp_part {
                    Some(ts) => row![ts, msg_text].into(),
                    None => msg_text,
                };
                
                col.push(line)
            })
        })
        .unwrap_or_else(|| column![crate::theme::text_muted("No logs yet.", ctx)]);

    let log_area = scrollable(
        container(log_content)
            .width(Length::Fill)
            .padding([4, if is_compact { 6 } else { 10 }]),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .anchor_bottom()
    .style(move |t, s| crate::theme::scrollable_style(&palette, t, s));

    let log_container = container(log_area)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |t| crate::theme::glass_container(&palette, t));

    // ── Clear button for current tab ─────────────────────────────────────────
    let btn_clear = button(crate::theme::text_muted(if is_compact { "⌫" } else { "Clear" }, ctx))
        .padding([4, if is_compact { 8 } else { 10 }])
        .on_press(Message::Server(ServerMessage::ClearCurrentTab))
        .style(move |t, s| crate::theme::secondary_button_style(&palette, t, s));

    // ── Header row ───────────────────────────────────────────────────────────
    let header = row![
        crate::theme::text(
            text(if is_compact { 
                localization.t("logs.title") 
            } else { 
                localization.t("logs.panel") 
            }).size(if is_compact { 14 } else { 16 }), 
            ctx
        ),
        Space::new().width(Length::Fill),
        btn_clear,
        button(crate::theme::text_muted("✕", ctx))
            .padding([6, 10])
            .on_press(Message::CloseServerPanel)
            .style(move |t, s| crate::theme::icon_button_style(&palette, t, s)),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    // ── Assemble ──────────────────────────────────────────────────────────────
    let panel = column![
        header,
        tab_buttons,
        log_container,
        controls,
    ]
    .spacing(if is_compact { 6 } else { 10 })
    .padding(padding)
    .width(Length::Fill)
    .height(Length::Fill);

    // IMPORTANT: Wrap in mouse_area to capture all clicks and prevent them
    // from passing through to the underlying UI. We use Message::None which
    // is a no-op message that gets ignored by the update function.
    mouse_area(
        container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |t| crate::theme::modal_container(&palette, t))
    )
    .on_press(Message::None)
    .on_release(Message::None)
    .into()
}

/// Get tab label with optional count indicator
fn get_tab_label_with_count(state: &ServerPanelState, tab: &LogTab, localization: &rustale_shared::lang::Localization, _ctx: UIContext) -> (String, usize) {
    let count = state.logs.get(tab).map(|l| l.len()).unwrap_or(0);
    (localization.t(tab.label_key()).to_string(), if count > 0 { count.min(999) } else { 0 })
}

/// Render tab-specific controls with responsive design
fn view_tab_controls<'a>(
    state: &'a ServerPanelState,
    localization: &'a rustale_shared::lang::Localization,
    ctx: UIContext, 
    is_compact: bool
) -> Element<'a, Message> {
    let palette = ctx.palette;

    match state.active_tab {
        LogTab::Launcher | LogTab::GameClient | LogTab::AuthServer => {
            // Read-only tabs - no controls, just info
            row![
                crate::theme::text_caption("Read-only logs", ctx),
                Space::new().width(Length::Fill),
            ]
            .spacing(8)
            .into()
        }
        LogTab::GameServer => {
            // Game server controls - use palette colors for state
            let (state_label, state_color) = match &state.server_state {
                ServerState::Idle => ("●", palette.text_secondary),
                ServerState::Starting => ("◌", palette.accent),
                ServerState::Running => ("●", palette.success),
                ServerState::Stopping => ("◌", Color::from_rgb(1.0, 0.6, 0.1)),
                ServerState::Stopped { .. } => ("■", palette.text_secondary),
                ServerState::Error(_) => ("✕", palette.danger),
            };

            // In compact mode, show shorter label or just icon
            let full_label = match &state.server_state {
                ServerState::Idle => localization.t("server.status_idle"),
                ServerState::Starting => localization.t("server.status_starting"),
                ServerState::Running => localization.t("server.status_running"),
                ServerState::Stopping => localization.t("server.status_stopping"),
                ServerState::Stopped { .. } => localization.t("server.status_stopped"),
                ServerState::Error(_) => localization.t("server.status_error"),
            };

            let state_badge = container(
                if is_compact {
                    crate::theme::text(
                        text(state_label).size(12).color(state_color),
                        ctx,
                    )
                } else {
                    crate::theme::text(
                        text(format!("{} {}", state_label, full_label)).size(12).color(state_color),
                        ctx,
                    )
                }
            )
            .padding([4, if is_compact { 6 } else { 10 }])
            .style(move |_| container::Style {
                background: Some(Color { a: 0.15 * palette.background.a, ..state_color }.into()),
                border: iced::Border {
                    color: Color { a: 0.4 * palette.background.a, ..state_color },
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            });

            let is_running = matches!(state.server_state, ServerState::Running);
            let is_busy = matches!(state.server_state, ServerState::Starting | ServerState::Stopping);
            let has_manager = state.manager.is_some();

            // In compact mode, use icon-only buttons
            let btn_start = button(crate::theme::text_small(if is_compact { "▶" } else { "▶ Start" }, ctx))
                .padding([5, if is_compact { 8 } else { 12 }])
                .on_press_maybe(if has_manager && !is_running && !is_busy {
                    Some(Message::Server(ServerMessage::Start))
                } else {
                    None
                })
                .style(move |t, s| crate::theme::success_button_style(&palette, t, s));

            let btn_stop = button(crate::theme::text_small(if is_compact { "■" } else { "■ Stop" }, ctx))
                .padding([5, if is_compact { 8 } else { 12 }])
                .on_press_maybe(if is_running {
                    Some(Message::Server(ServerMessage::Stop))
                } else {
                    None
                })
                .style(move |t, s| crate::theme::danger_button_style(&palette, t, s));

            let btn_restart = button(crate::theme::text_small(if is_compact { "↺" } else { "↺ Restart" }, ctx))
                .padding([5, if is_compact { 8 } else { 12 }])
                .on_press_maybe(if has_manager && !is_busy {
                    Some(Message::Server(ServerMessage::Restart))
                } else {
                    None
                })
                .style(move |t, s| crate::theme::secondary_button_style(&palette, t, s));

            let cmd_input = text_input(if is_compact { "cmd..." } else { "Send command..." }, &state.command_input)
                .size(if is_compact { 11 } else { 12 })
                .font(iced::Font::MONOSPACE)
                .on_input(|s| Message::Server(ServerMessage::CommandInput(s)))
                .on_submit(Message::Server(ServerMessage::SendCommand))
                .padding([6, 8])
                .width(Length::Fill)
                .style(move |t, s| crate::theme::text_input_style(&palette, t, s));

            // Send button with paper plane icon in compact mode
            let btn_send = button(crate::theme::text_small(if is_compact { "➤" } else { "Send" }, ctx))
                .padding([6, if is_compact { 10 } else { 14 }])
                .on_press_maybe(if is_running && !state.command_input.trim().is_empty() {
                    Some(Message::Server(ServerMessage::SendCommand))
                } else {
                    None
                })
                .style(move |t, s| crate::theme::primary_button_style(&palette, t, s));

            row![
                state_badge,
                btn_start,
                btn_stop,
                btn_restart,
                Space::new().width(Length::Fill),
                cmd_input,
                btn_send,
            ]
            .spacing(if is_compact { 4 } else { 8 })
            .align_y(iced::Alignment::Center)
            .into()
        }
    }
}
