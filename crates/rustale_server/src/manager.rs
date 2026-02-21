/// Managed dedicated-server abstraction.
///
/// `ServerManager` lets any consumer (CLI, GUI panel, TUI, ...) control the
/// server lifecycle and subscribe to its log stream without touching any of
/// the low-level Tokio/process plumbing.
///
/// # Cloning
/// `ServerManager` is cheaply `Clone` — all state lives behind `Arc`s, so
/// every clone refers to the same underlying process controller.
///
/// # Example – embed in a GUI
/// ```rust
/// let mgr = ServerManager::new(config);
/// let mut events = mgr.subscribe();
///
/// // later, in an iced subscription:
/// while let Ok(evt) = events.recv().await {
///     match evt {
///         ServerEvent::LogLine(l) => update_log_panel(l),
///         ServerEvent::StateChanged(s) => update_status_badge(s),
///         _ => {}
///     }
/// }
/// ```
use crate::config::ServerConfig;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, oneshot, RwLock};

// ─── Public types ────────────────────────────────────────────────────────────

/// Lifecycle state of the dedicated server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerState {
    /// Manager created, server has not been started yet.
    Idle,
    /// Server is going through its setup / patching / download phase.
    Starting,
    /// Java process is up and accepting connections.
    Running,
    /// Stop was requested; waiting for the process to exit cleanly.
    Stopping,
    /// Process has terminated.
    Stopped {
        /// `None` if killed by signal.
        exit_code: Option<i32>,
    },
    /// An unrecoverable error occurred (see message).
    Error(String),
}

impl std::fmt::Display for ServerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "Idle"),
            Self::Starting => write!(f, "Starting..."),
            Self::Running => write!(f, "Running"),
            Self::Stopping => write!(f, "Stopping..."),
            Self::Stopped { exit_code: Some(c) } => write!(f, "Stopped (exit {})", c),
            Self::Stopped { exit_code: None } => write!(f, "Stopped"),
            Self::Error(e) => write!(f, "Error: {}", e),
        }
    }
}

/// Events produced by the server that consumers can react to.
#[derive(Debug, Clone)]
pub enum ServerEvent {
    /// The server lifecycle state changed.
    StateChanged(ServerState),
    /// A line from the server's stdout.
    LogLine(String),
    /// A line from the server's stderr.
    ErrLine(String),
}

// ─── Internal ────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub(crate) enum ServerControl {
    Start,
    Stop,
    Restart,
    /// Send a text command to the server's stdin (e.g. `"stop"`, `"list"`).
    SendStdin(String),
}

// ─── ServerManager ───────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ServerManager {
    config: Arc<RwLock<ServerConfig>>,
    state: Arc<RwLock<ServerState>>,
    event_tx: broadcast::Sender<ServerEvent>,
    control_tx: mpsc::Sender<ServerControl>,
}

impl ServerManager {
    /// Create a manager and start its background controller.
    pub fn new(config: ServerConfig) -> Self {
        let (event_tx, _) = broadcast::channel(1024);
        let (control_tx, control_rx) = mpsc::channel::<ServerControl>(32);
        let state = Arc::new(RwLock::new(ServerState::Idle));
        let config = Arc::new(RwLock::new(config));

        let mgr = Self {
            config: config.clone(),
            state: state.clone(),
            event_tx: event_tx.clone(),
            control_tx,
        };

        tokio::spawn(controller_loop(state, config, event_tx, control_rx));

        mgr
    }

    /// Subscribe to the event stream.
    /// Every subscriber gets its own independent queue – falling behind one
    /// consumer does not stall others (broadcast semantics).
    pub fn subscribe(&self) -> broadcast::Receiver<ServerEvent> {
        self.event_tx.subscribe()
    }

    /// Start the server. No-op if already running or starting.
    pub async fn start(&self) -> Result<()> {
        self.control_tx
            .send(ServerControl::Start)
            .await
            .map_err(|_| anyhow::anyhow!("Server controller has shut down"))
    }

    /// Gracefully stop the server.
    pub async fn stop(&self) -> Result<()> {
        self.control_tx
            .send(ServerControl::Stop)
            .await
            .map_err(|_| anyhow::anyhow!("Server controller has shut down"))
    }

    /// Stop (if running) then start again.
    pub async fn restart(&self) -> Result<()> {
        self.control_tx
            .send(ServerControl::Restart)
            .await
            .map_err(|_| anyhow::anyhow!("Server controller has shut down"))
    }

    /// Send a command line to the server's stdin.
    pub async fn send_command(&self, cmd: String) -> Result<()> {
        self.control_tx
            .send(ServerControl::SendStdin(cmd))
            .await
            .map_err(|_| anyhow::anyhow!("Server controller has shut down"))
    }

    /// Async snapshot of the current state.
    pub async fn current_state(&self) -> ServerState {
        self.state.read().await.clone()
    }

    /// Synchronous snapshot – useful inside iced `view()` / `update()`.
    /// Falls back to `Idle` if the lock is contended (extremely rare).
    pub fn current_state_sync(&self) -> ServerState {
        self.state
            .try_read()
            .map(|g| g.clone())
            .unwrap_or(ServerState::Idle)
    }

    /// Swap the configuration used for the next `start()` / `restart()`.
    pub async fn update_config(&self, config: ServerConfig) {
        *self.config.write().await = config;
    }
}

// ─── Controller loop ─────────────────────────────────────────────────────────

async fn controller_loop(
    state: Arc<RwLock<ServerState>>,
    config: Arc<RwLock<ServerConfig>>,
    event_tx: broadcast::Sender<ServerEvent>,
    mut control_rx: mpsc::Receiver<ServerControl>,
) {
    // These are valid only while a server run is active.
    let mut stop_tx: Option<oneshot::Sender<()>> = None;
    let mut stdin_tx: Option<mpsc::Sender<String>> = None;

    while let Some(cmd) = control_rx.recv().await {
        match cmd {
            ServerControl::Start => {
                {
                    let s = state.read().await;
                    if matches!(*s, ServerState::Running | ServerState::Starting) {
                        continue;
                    }
                }
                do_start(&state, &config, &event_tx, &mut stop_tx, &mut stdin_tx).await;
            }
            ServerControl::Stop => {
                do_stop(&state, &event_tx, &mut stop_tx, &mut stdin_tx).await;
            }
            ServerControl::Restart => {
                do_stop(&state, &event_tx, &mut stop_tx, &mut stdin_tx).await;
                // Brief delay so the process has a chance to exit.
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                do_start(&state, &config, &event_tx, &mut stop_tx, &mut stdin_tx).await;
            }
            ServerControl::SendStdin(line) => {
                if let Some(ref tx) = stdin_tx {
                    let _ = tx.send(line).await;
                }
            }
        }
    }
}

async fn do_start(
    state: &Arc<RwLock<ServerState>>,
    config: &Arc<RwLock<ServerConfig>>,
    event_tx: &broadcast::Sender<ServerEvent>,
    stop_tx: &mut Option<oneshot::Sender<()>>,
    stdin_tx: &mut Option<mpsc::Sender<String>>,
) {
    set_state(state, event_tx, ServerState::Starting).await;

    let cfg = config.read().await.clone();
    let (otx, orx) = oneshot::channel::<()>();
    let (cmd_tx, cmd_rx) = mpsc::channel::<String>(64);
    *stop_tx = Some(otx);
    *stdin_tx = Some(cmd_tx);

    let sink = crate::runner::LogSink::Channel(event_tx.clone());
    let state_clone = Arc::clone(state);
    let event_clone = event_tx.clone();

    tokio::spawn(async move {
        let result = crate::runner::run_server_flow_internal(cfg, sink, orx, cmd_rx).await;
        let final_state = match result {
            Ok(exit_code) => ServerState::Stopped { exit_code },
            Err(e) => {
                // Also emit as an ErrLine so consumers see it in their log view
                let _ = event_clone.send(ServerEvent::ErrLine(
                    format!("[FATAL] {}", e),
                ));
                ServerState::Error(e.to_string())
            }
        };
        set_state(&state_clone, &event_clone, final_state).await;
    });
}

async fn do_stop(
    state: &Arc<RwLock<ServerState>>,
    event_tx: &broadcast::Sender<ServerEvent>,
    stop_tx: &mut Option<oneshot::Sender<()>>,
    stdin_tx: &mut Option<mpsc::Sender<String>>,
) {
    // Drop the stdin channel so the server side sees EOF.
    *stdin_tx = None;

    if let Some(tx) = stop_tx.take() {
        set_state(state, event_tx, ServerState::Stopping).await;
        let _ = tx.send(());
    }
}

async fn set_state(
    state: &Arc<RwLock<ServerState>>,
    event_tx: &broadcast::Sender<ServerEvent>,
    new_state: ServerState,
) {
    *state.write().await = new_state.clone();
    let _ = event_tx.send(ServerEvent::StateChanged(new_state));
}
