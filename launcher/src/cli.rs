use clap::Parser;
use rustale_shared::config::OnlineFixMode;
use rustale_engine::util;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Start in Quickplay mode (no UI)
    #[arg(long)]
    pub quickplay: bool,

    // --- SERVER ARGUMENTS ---
    /// Enable Dedicated Server Mode (CLI only)
    #[arg(long)]
    pub dedicated_server: bool,

    /// Online Mode: local or sanasol
    #[arg(long)]
    pub online_mode: Option<String>,

    /// Update Branch: release or pre-release
    #[arg(long)]
    pub branch: Option<String>,

    /// Game Version: latest, 5, etc.
    #[arg(long)]
    pub game_version: Option<String>,

    /// Server Args
    #[arg(long)]
    pub server_args: Option<String>,

    /// Java Exec Args
    #[arg(long)]
    pub java_exec_args: Option<String>,

    /// Tunnel Provider
    #[arg(long)]
    pub tunnel: Option<String>,

    /// Force close existing instance
    #[arg(long)]
    pub force: bool,
}

pub fn parse() -> Args {
    Args::parse()
}

/// Executes the proxy logic and terminates the process
pub fn run_proxy() -> std::process::ExitCode {
    let mode_env = std::env::var("AURORA_MODE").unwrap_or_default();
    let mode = match mode_env.as_str() {
        "sanasol" => OnlineFixMode::Sanasol,
        _ => OnlineFixMode::Local,
    };

    if let Err(e) = util::run_java_proxy_logic(mode) {
        eprintln!("Java Proxy Error: {}", e);
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}
