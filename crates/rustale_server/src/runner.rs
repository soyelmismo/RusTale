use crate::assets::{
    find_best_client_version, generate_server_args_with_direct_assets, validate_client_version,
};
use crate::config::ServerConfig;
use crate::manager::ServerEvent;
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::{broadcast, mpsc, oneshot};
use rustale_engine::game::patch_api::PatchApiFrontend;
use rustale_engine::game::progress::ProgressCallback;

// ─── LogSink ─────────────────────────────────────────────────────────────────

/// Where log lines produced by the server flow are sent.
///
/// - `Console` → classic `println!` / `eprintln!` (CLI / dedicated-server binary).
/// - `Channel` → broadcast channel (GUI panel, TUI attach, test harness, ...).
///
/// Both variants are `Clone` so they can be handed to spawned tasks.
#[derive(Clone)]
pub enum LogSink {
    /// Print directly to the process stdout/stderr.
    Console,
    /// Forward to a broadcast channel as `ServerEvent` variants.
    Channel(broadcast::Sender<ServerEvent>),
}

impl LogSink {
    /// Emit a stdout-style log line.
    pub fn log(&self, line: impl Into<String>) {
        let line = line.into();
        match self {
            Self::Console => println!("{}", line),
            Self::Channel(tx) => {
                let _ = tx.send(ServerEvent::LogLine(line));
            }
        }
    }

    /// Emit a stderr-style log line.
    pub fn err(&self, line: impl Into<String>) {
        let line = line.into();
        match self {
            Self::Console => eprintln!("{}", line),
            Self::Channel(tx) => {
                let _ = tx.send(ServerEvent::ErrLine(line));
            }
        }
    }

    /// Emit a state-change event (only meaningful for `Channel` mode).
    pub fn state(&self, s: crate::manager::ServerState) {
        if let Self::Channel(tx) = self {
            let _ = tx.send(ServerEvent::StateChanged(s));
        }
    }

    /// Returns `true` when running in managed (channel) mode.
    #[inline]
    pub fn is_managed(&self) -> bool {
        matches!(self, Self::Channel(_))
    }
}

// ─── CLI-compatible entry point ───────────────────────────────────────────────

/// Thin CLI wrapper — preserves the original public API.
///
/// Creates an internal oneshot/channel pair, forwards Ctrl-C to the stop
/// signal, and pipes the local terminal's stdin into the Java process so that
/// operators can type commands (e.g. `"stop"`, `"list"`) directly.
pub async fn run_server_flow(config: ServerConfig) -> Result<()> {
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let (stdin_tx, stdin_rx) = mpsc::channel::<String>(64);

    // Forward Ctrl-C → stop_rx
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            let _ = stop_tx.send(());
        }
    });

    // Forward terminal stdin → Java stdin (via channel)
    tokio::spawn(async move {
        use tokio::io::AsyncBufReadExt;
        let mut reader = BufReader::new(tokio::io::stdin()).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            if stdin_tx.send(line).await.is_err() {
                break;
            }
        }
    });

    run_server_flow_internal(config, LogSink::Console, stop_rx, stdin_rx).await?;
    Ok(())
}

// ─── Core server flow ─────────────────────────────────────────────────────────

/// Run the full dedicated-server lifecycle.
///
/// All user-visible output goes through `sink` so the caller decides whether
/// it ends up on stdout or in a broadcast channel.
///
/// # Parameters
/// - `config`   – server configuration (cloned from `ServerManager` or built by CLI).
/// - `sink`     – where log lines are emitted.
/// - `stop_rx`  – resolves when an external stop request arrives.
/// - `stdin_rx` – lines forwarded to the Java process's stdin.
///
/// # Returns
/// `Ok(Some(exit_code))` on a clean exit, `Ok(None)` if killed by stop signal,
/// `Err(...)` on a fatal setup error.
pub async fn run_server_flow_internal(
    mut config: ServerConfig,
    sink: LogSink,
    mut stop_rx: oneshot::Receiver<()>,
    mut stdin_rx: mpsc::Receiver<String>,
) -> Result<Option<i32>> {
    sink.log("--- RusTale Dedicated Server ---");
    sink.log(format!(
        "Mode: {} | Port: 5520 | Version: {}",
        config.online_mode, config.game_version
    ));

    // ── Auth port discovery ──────────────────────────────────────────────────
    let mut auth_port = rustale_engine::util::get_saved_port();

    if auth_server::is_server_alive(auth_port).await {
        sink.log(format!("[RusTale] Local Auth found on port {}", auth_port));
        sink.log("[RusTale] Attaching to existing emulator instance...");
    } else {
        auth_port = rustale_engine::util::find_free_port();
        sink.log(format!(
            "[RusTale] No active instance found. Starting new emulator on port {}",
            auth_port
        ));
        rustale_engine::util::save_active_port(auth_port);
    }

    // ── Directory setup ──────────────────────────────────────────────────────
    let root_dir = rustale_shared::config::get_server_root_dir();
    let _ = tokio::fs::create_dir_all(&root_dir).await;
    let _ = auth_server::crypto::set_identity_dir(rustale_shared::config::get_identity_dir());

    // ── Auth server startup ──────────────────────────────────────────────────
    let (auth_result_tx, mut auth_result_rx) = tokio::sync::oneshot::channel();
    let (_server_stop_tx, server_stop_rx) = tokio::sync::oneshot::channel::<()>();

    if config.online_mode == "local" {
        if auth_server::is_server_alive(auth_port).await {
            sink.log(format!(
                "Local Auth Server already running on port {}. Attaching...",
                auth_port
            ));

            let auth_url = format!("http://127.0.0.000001:{}", auth_port);
            match rustale_engine::game::auth::fetch_remote_jwks(&*rustale_shared::HTTP_CLIENT, &auth_url).await {
                Ok(jwks) => {
                    auth_server::crypto::update_jwks_from_remote(jwks);
                }
                Err(e) => {
                    sink.err(format!(
                        "[Server] Warning: Could not sync JWKS from existing emulator: {}",
                        e
                    ));
                }
            }

            let _ = auth_result_tx.send(Ok(()));
        } else {
            sink.log(format!(
                "Starting Local Auth Server (Emulator) on port {}...",
                auth_port
            ));

            let host_uuid = uuid::Uuid::new_v4().to_string();
            let host_name = "ConsoleHost".to_string();

            let version_dir_name = if config.game_version == "latest" || config.game_version == "0" {
                "latest".to_string()
            } else {
                config.game_version.clone()
            };
            let install_dir_ref = root_dir.join(&config.branch).join(&version_dir_name);

            tokio::spawn(async move {
                let res = auth_server::start_server(
                    host_name,
                    host_uuid,
                    install_dir_ref,
                    rustale_shared::config::get_identity_dir(),
                    server_stop_rx,
                    auth_port,
                )
                .await;
                let _ = auth_result_tx.send(res);
            });

            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    } else {
        let _ = auth_result_tx.send(Ok(()));
    }

    if let Ok(Err(e)) = auth_result_rx.try_recv() {
        sink.err(format!("\n[FATAL] Could not start authentication server: {}", e));
        sink.err("It is likely that a previous instance was left open.");
        sink.err("Please close 'java' or 'rustale' from Task Manager.\n");
        return Err(anyhow::anyhow!("Auth server failed to start: {}", e));
    }

    // ── Path resolution ──────────────────────────────────────────────────────
    let _tools_dir = root_dir.join("tools");
    let version_dir_name = if config.game_version == "latest" || config.game_version == "0" {
        "latest".to_string()
    } else {
        config.game_version.clone()
    };
    let install_dir = root_dir.join(&config.branch).join(&version_dir_name);

    // ── DualAuth agent ───────────────────────────────────────────────────────
    sink.log("Ensuring DualAuth Agent is up to date...");
    rustale_engine::game::agent::ensure_agent(
        &root_dir,
        &|_phase: String, _pct: f64, _msg: String| {},
        None,
    )
    .await?;

    // ── Tool validation ──────────────────────────────────────────────────────
    sink.log("[1/5] Validating tools...");

    let sink_for_callback = sink.clone();
    let callback: ProgressCallback = Arc::new(move |task: String,
                    pct: f64,
                    msg: String,
                    total: u64,
                    downloaded: u64,
                    eta: Option<String>,
                    _step: Option<usize>| {
        let eta_str = eta.as_ref().map(|e| format!(" • ETA: {}", e)).unwrap_or_default();
        if total > 0 {
            let size_info = format!(
                "{}/{}",
                rustale_engine::game::patch_api::utils::format_bytes(downloaded),
                rustale_engine::game::patch_api::utils::format_bytes(total)
            );
            sink_for_callback.log(format!("[{}] {:.1}% - {} ({}){}", task, pct, msg, size_info, eta_str));
        } else {
            sink_for_callback.log(format!("[{}] {:.1}% - {}{}", task, pct, msg, eta_str));
        }
    });

    // Re-use tools from main installation where possible.
    let main_app_dir = rustale_shared::config::get_app_dir();
    let tools_dir = main_app_dir.join("tools");
    let main_java = tools_dir.join("jre");
    let main_butler = tools_dir.join("butler");
    let server_java = _tools_dir.join("jre");
    let server_butler = _tools_dir.join("butler");

    if main_java.exists()
        && (!server_java.exists() || rustale_engine::java::get_java_exec(&root_dir).is_err())
    {
        sink.log("[Tools] Copying JRE from main installation...");
        let mr = main_java.clone();
        let sr = server_java.clone();
        tokio::task::spawn_blocking(move || rustale_engine::util::copy_recursive_sync(mr, sr)).await??;
    }

    if main_butler.exists() && !server_butler.exists() {
        sink.log("[Tools] Copying Butler from main installation...");
        let mt = main_butler.clone();
        let st = server_butler.clone();
        tokio::task::spawn_blocking(move || rustale_engine::util::copy_recursive_sync(mt, st)).await??;
    }

    let java_info = rustale_engine::java::detection::ensure_java_available(&root_dir).await?;
    sink.log(format!("[Server] Environment verified: Java {}", java_info.version));
    let patch_frontend = PatchApiFrontend::get_instance();
    let callback_clone = callback.clone();
    let _butler_path = patch_frontend
        .install_butler(&root_dir, callback_clone, None)
        .await?;

    // ── Game-file resolution ─────────────────────────────────────────────────
    sink.log("[2/5] Checking Game Server files...");

    let version_info = patch_frontend
        .get_version_info(&main_app_dir, &config.branch, 0)
        .await?;

    let target_ver_num = if config.game_version == "latest" || config.game_version == "0" {
        version_info.latest_remote
    } else {
        config
            .game_version
            .parse::<i32>()
            .context("Invalid version number")?
    };

    let server_jar_raw = install_dir.join("Server").join("HytaleServer.jar");

    if !server_jar_raw.exists() {
        sink.log("Checking for local game files in main installation...");
        let mut source_candidate: Option<PathBuf> = None;

        let version_str = if config.game_version == "latest" || config.game_version == "0" {
            "latest".to_string()
        } else {
            config.game_version.clone()
        };
        let specific = main_app_dir.join(&config.branch).join(&version_str);
        if specific.exists() && specific.join("Server").exists() {
            source_candidate = Some(specific);
        }

        if source_candidate.is_none() && version_str == "latest" {
            let latest = main_app_dir.join(&config.branch).join("latest");
            if latest.exists() {
                if let Ok(ver) =
                    rustale_engine::game::get_local_version(&main_app_dir, &config.branch).await
                {
                    if ver.to_string() == config.game_version {
                        source_candidate = Some(latest);
                    }
                }
            }
        }

        if let Some(src) = source_candidate {
            sink.log(format!(
                "Found matching version {} at {:?}. Processing files...",
                version_str, src
            ));
            let _ = tokio::fs::create_dir_all(&install_dir).await;

            let src_assets = src.join("Assets.zip");
            if config.use_direct_assets {
                if let Err(e) = validate_client_version(&main_app_dir, &config.branch, &version_str) {
                    sink.err(format!("Client validation failed: {}", e));
                    sink.err("Falling back to copying Assets.zip...");
                    let dst_assets = install_dir.join("Assets.zip");
                    if src_assets.exists() && !dst_assets.exists() {
                        sink.log("Copying Assets.zip (~3GB) as fallback...");
                        if let Err(e) = tokio::fs::copy(&src_assets, &dst_assets).await {
                            sink.err(format!("Failed to copy Assets.zip: {}", e));
                        }
                    }
                    config.server_args =
                        generate_server_args_with_direct_assets(&config.server_args, &dst_assets);
                } else {
                    sink.log("Using assets directly from client installation (no copying)");
                    sink.log(format!("Assets path: {:?}", src_assets));
                    config.server_args =
                        generate_server_args_with_direct_assets(&config.server_args, &src_assets);
                }
            } else {
                let dst_assets = install_dir.join("Assets.zip");
                if src_assets.exists() && !dst_assets.exists() {
                    sink.log("Copying Assets.zip (~3GB)...");
                    if let Err(e) = tokio::fs::copy(&src_assets, &dst_assets).await {
                        sink.err(format!("Failed to copy Assets.zip: {}", e));
                    }
                } else if !src_assets.exists() {
                    sink.err(format!("WARNING: Source Assets.zip not found at {:?}", src_assets));
                    sink.err("This may cause Exit Code 7 - server will fail without assets!");
                }
                config.server_args =
                    generate_server_args_with_direct_assets(&config.server_args, &dst_assets);
            }

            let src_server = src.join("Server");
            let dst_server = install_dir.join("Server");
            if src_server.exists() && !dst_server.exists() {
                sink.log("Copying Server folder...");
                let dst_s_clone = dst_server.clone();
                let res = tokio::task::spawn_blocking(move || {
                    rustale_engine::util::copy_recursive_sync(src_server, dst_s_clone)
                })
                .await?;

                if let Err(e) = res {
                    sink.err(format!("Failed to copy Server folder: {}", e));
                } else {
                    sink.log("Files copied successfully!");
                }
            }
        }
    }

    // ── Asset configuration ──────────────────────────────────────────────────
    sink.log("Setting up assets configuration...");
    let local_assets_path = install_dir.join("Assets.zip");
    let mut source_candidate: Option<PathBuf> = None;

    let main_app_dir = rustale_shared::config::get_app_dir();
    let version_str = match find_best_client_version(
        &main_app_dir,
        &config.branch,
        &config.game_version.to_string(),
    ) {
        Ok(version) => {
            sink.log(format!("Found matching client version for assets: {}", version));
            version
        }
        Err(e) => {
            sink.log(format!("Warning: {}", e));
            config.game_version.clone()
        }
    };

    if local_assets_path.exists() {
        sink.log("[Assets] Using existing Assets.zip found in server directory.");
        config.server_args =
            generate_server_args_with_direct_assets(&config.server_args, &local_assets_path);
    } else {
        sink.log("Assets not found in server dir, searching client installation...");

        let specific = main_app_dir.join(&config.branch).join(&version_str);
        if specific.exists() && specific.join("Assets.zip").exists() {
            source_candidate = Some(specific);
        }

        if source_candidate.is_none() && version_str == "latest" {
            let latest = main_app_dir.join(&config.branch).join("latest");
            if latest.exists() && latest.join("Assets.zip").exists() {
                source_candidate = Some(latest);
            }
        }

        if let Some(src) = source_candidate {
            let src_assets = src.join("Assets.zip");
            sink.log(format!("Found client assets at: {:?}", src_assets));

            if config.use_direct_assets {
                if let Err(e) = validate_client_version(&main_app_dir, &config.branch, &version_str) {
                    sink.err(format!("Client validation failed: {}", e));
                    sink.err("Falling back to copying Assets.zip...");
                    if src_assets.exists() && !local_assets_path.exists() {
                        sink.log("Copying Assets.zip (~3GB) as fallback...");
                        if let Err(e) = tokio::fs::copy(&src_assets, &local_assets_path).await {
                            sink.err(format!("Failed to copy Assets.zip: {}", e));
                        }
                    }
                    config.server_args = generate_server_args_with_direct_assets(
                        &config.server_args,
                        &local_assets_path,
                    );
                } else {
                    sink.log("Using assets directly from client installation (no copying)");
                    sink.log(format!("Assets path: {:?}", src_assets));
                    config.server_args =
                        generate_server_args_with_direct_assets(&config.server_args, &src_assets);
                }
            } else {
                if src_assets.exists() && !local_assets_path.exists() {
                    sink.log("Copying Assets.zip (~3GB)...");
                    if let Err(e) = tokio::fs::copy(&src_assets, &local_assets_path).await {
                        sink.err(format!("Failed to copy Assets.zip: {}", e));
                    }
                } else if !src_assets.exists() {
                    sink.err(format!("WARNING: Source Assets.zip not found at {:?}", src_assets));
                    sink.err("This may cause Exit Code 7 - server will fail without assets!");
                }
                config.server_args = generate_server_args_with_direct_assets(
                    &config.server_args,
                    &local_assets_path,
                );
            }
        } else {
            sink.err("WARNING: No client assets found! Server may fail to start.");
            sink.err(format!(
                "Looking for assets in: {:?}",
                main_app_dir
                    .join(&config.branch)
                    .join(&version_str)
                    .join("Assets.zip")
            ));
        }
    }

    // ── Download if nothing is available ────────────────────────────────────
    let jar_available = server_jar_raw.exists();
    let assets_exist_in_server = local_assets_path.exists();
    let assets_exist_in_client = {
        let specific = main_app_dir.join(&config.branch).join(&version_str);
        if specific.exists() && specific.join("Assets.zip").exists() {
            true
        } else if version_str == "latest" {
            let latest = main_app_dir.join(&config.branch).join("latest");
            latest.exists() && latest.join("Assets.zip").exists()
        } else {
            false
        }
    };

    if !jar_available && (!assets_exist_in_server && !assets_exist_in_client) {
        sink.log(format!(
            "[Server] No assets found. Downloading version {}...",
            target_ver_num
        ));

        let cache = rustale_engine::game::patch_api::get_shared_cache();
        let callback_for_pwr = callback.clone();
        let localization = rustale_engine::lang::Localization::new();
        let patch_path = cache
            .get_or_download_patch(
                &config.branch,
                0,
                target_ver_num,
                callback,
                None,
                &localization,
            )
            .await?;

        sink.log("[Server] Applying patch via Butler...");

        let localization = rustale_engine::lang::Localization::new();
        rustale_engine::game::patcher::apply_pwr(
            &root_dir,
            &config.branch,
            &version_dir_name,
            &patch_path,
            callback_for_pwr,
            None,
            &localization,
        )
        .await?;

        config.server_args =
            generate_server_args_with_direct_assets(&config.server_args, &local_assets_path);
    } else {
        if jar_available {
            sink.log("[Skip] Base JAR is already available locally. Proceeding to patch check.");
        } else {
            sink.log(format!(
                "[Skip] Assets are available locally (server: {}, client: {}). Proceeding with existing files.",
                assets_exist_in_server, assets_exist_in_client
            ));
        }
    }

    // ── Runtime preparation ──────────────────────────────────────────────────
    let target_jar_path = server_jar_raw.clone();
    sink.log(format!("Using JAR: {:?}", target_jar_path));

    sink.log("[4/5] Preparing Runtime...");

    let java_exec = rustale_engine::java::get_java_exec(&root_dir)?;
    let java_path = PathBuf::from(&java_exec);

    // Siempre parchar Java (tanto singleplayer como servidor)
    // Usar spawn_blocking para no bloquear el hilo async
    let java_path_clone = java_path.clone();
    let proxy_setup = tokio::task::spawn_blocking(move || {
        rustale_engine::java::proxy::setup_java_proxy(&java_path_clone)
    }).await;
    
    if let Ok(Err(e)) = proxy_setup {
        sink.err(format!("[Runner] Warning: Failed to setup Java proxy: {}", e));
    }
    
    // Servidor dedicado usa java_original explícitamente
    let java_original_name = if cfg!(windows) { "java_original.exe" } else { "java_original" };
    let java_original_path = java_path.parent().unwrap().join(java_original_name);
    
    let final_java = if java_original_path.exists() {
        java_original_path.to_string_lossy().to_string()
    } else {
        sink.log("[Runner] (Server) Warning: java_original not found, using vanilla java");
        java_exec.clone()
    };
    
    sink.log(format!("[Runner] (Server) Using Java: {}", final_java));

    rustale_engine::util::make_executable(&PathBuf::from(&final_java)).await?;

    // ── Tunnel ───────────────────────────────────────────────────────────────
    if let Some(provider) = &config.tunnel_provider {
        if provider == "playit" {
            let root_clone = root_dir.clone();
            let (tx, mut rx) = mpsc::channel(1);

            sink.log("Starting tunnel and waiting for connection details...");

            tokio::spawn(async move {
                if let Err(e) = crate::tunnel::start_playit(&root_clone, tx).await {
                    eprintln!("Tunnel Error: {}", e);
                }
            });

            let timeout = tokio::time::sleep(tokio::time::Duration::from_secs(60));

            tokio::select! {
                res = rx.recv() => {
                    match res {
                        Some(msg) => {
                            sink.log("\n---------------------------------------------------");
                            sink.log(format!("TUNNEL STATUS: {}", msg));
                            sink.log("---------------------------------------------------\n");
                        }
                        None => sink.log("Tunnel closed unexpectedly."),
                    }
                }
                _ = timeout => {
                    sink.log("WARNING: Tunnel took too long to negotiate. Starting server anyway.");
                }
            }
        }
    }

    // ── Pre-launch validation ────────────────────────────────────────────────
    sink.log("[5/5] Launching Server on port 5520!");
    sink.log("---------------------------------------------------");

    sink.log("Final pre-launch validation:");
    sink.log(format!("  - Working directory: {:?}", install_dir));
    sink.log(format!("  - Server JAR: {:?}", target_jar_path));

    if !target_jar_path.exists() {
        sink.err(format!("FATAL: Patched JAR not found at {:?}", target_jar_path));
        return Err(anyhow::anyhow!(
            "Patched JAR missing - this will cause Exit Code 7"
        ));
    }

    let assets_in_install_dir = install_dir.join("Assets.zip");
    if assets_in_install_dir.exists() {
        sink.log(format!("  - Assets.zip: {:?} (local copy)", assets_in_install_dir));
    } else {
        sink.log("  - Assets.zip: Using direct assets path");
    }

    let server_dir = install_dir.join("Server");
    if !server_dir.exists() {
        sink.err(format!("WARNING: Server directory not found at {:?}", server_dir));
    }

    let clean_install_dir = rustale_engine::util::sanitize_path(&install_dir);
    let clean_target_jar = rustale_engine::util::sanitize_path(&target_jar_path);

    sink.log(format!("  - Final server args: {}", config.server_args));

    // ── Build command ────────────────────────────────────────────────────────
    let mut cmd = Command::new(final_java);

    cmd.env("AURORA_MODE", &config.online_mode);
    cmd.current_dir(&clean_install_dir);

    if let Some(domain) = &config.auth_domain {
        if domain == "127.0.0.000001" {
            cmd.env(
                "HYTALE_AUTH_DOMAIN",
                format!("127.0.0.000001:{}", auth_port),
            );
        } else {
            cmd.env("HYTALE_AUTH_DOMAIN", domain);
        }
    } else {
        if config.online_mode == "local" {
            cmd.env(
                "HYTALE_AUTH_DOMAIN",
                format!("127.0.0.000001:{}", auth_port),
            );
        } else if config.online_mode == "sanasol" {
            cmd.env("HYTALE_AUTH_DOMAIN", "sessions.sanasol.ws");
        }
    }

    cmd.env("DISABLE_SENTRY", "1");
    cmd.env("HYTALE_TRUST_ALL_ISSUERS", config.trust_all_issuers.to_string());
    if !config.trusted_issuers.is_empty() {
        cmd.env("HYTALE_TRUSTED_ISSUERS", config.trusted_issuers.join(","));
    }

    let agent_path = rustale_engine::game::paths::GamePaths::new(root_dir.clone()).dualauth_agent();
    if agent_path.exists() {
        let agent_arg = format!("-javaagent:{}", agent_path.to_string_lossy());
        let current_args = cmd.as_std().get_args().collect::<Vec<&std::ffi::OsStr>>();
        let already_present = current_args.iter().any(|a| a.to_string_lossy() == agent_arg);
        if !already_present {
            cmd.arg(agent_arg);
            sink.log(format!("[Server] Injecting Java Agent: {:?}", agent_path));
        } else {
            sink.log("[Server] Java Agent already present in arguments. Skipping injection.");
        }
    } else {
        sink.log(format!(
            "  - WARNING: Java Agent NOT FOUND at {:?}. Authentication may fail.",
            agent_path
        ));
    }

    let java_args: Vec<&str> = config.java_exec_args.split_whitespace().collect();
    cmd.args(java_args).arg("-jar").arg(&clean_target_jar);
    cmd.args(config.server_args.split_whitespace());
    cmd.arg("--disable-sentry");

    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    // ── Stdin mode ───────────────────────────────────────────────────────────
    // Managed mode: pipe stdin and forward from channel.
    // CLI mode: also pipe (terminal input comes via the forwarding task
    //           started in `run_server_flow`).
    cmd.stdin(std::process::Stdio::piped());

    cmd.env("RUSTALE_IS_SERVER", "1");

    #[cfg(windows)]
    {
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }

    cmd.kill_on_drop(true);

    let mut child = cmd.spawn().context("Failed to spawn java process")?;

    // ── Stdin forwarding ─────────────────────────────────────────────────────
    if let Some(mut child_stdin) = child.stdin.take() {
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            while let Some(line) = stdin_rx.recv().await {
                let _ = child_stdin.write_all(format!("{}\n", line).as_bytes()).await;
            }
        });
    }

    // ── Log streaming ────────────────────────────────────────────────────────
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let sink_out = sink.clone();
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            sink_out.log(line);
        }
    });

    // Emit a state change: the process is now up (Running).
    sink.state(crate::manager::ServerState::Running);

    let sink_err = sink.clone();
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            sink_err.err(line);
        }
    });

    // ── Windows Job Object ───────────────────────────────────────────────────
    #[cfg(windows)]
    let _job_guard = {
        use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_ALL_ACCESS};
        use windows_sys::Win32::Foundation::HANDLE;

        match rustale_shared::java::win_job::JobObject::new() {
            Ok(job) => {
                if let Some(pid) = child.id() {
                    unsafe {
                        let process_handle: HANDLE = OpenProcess(PROCESS_ALL_ACCESS, 0, pid);

                        if process_handle.is_null() {
                            sink.err(format!("[Warning] Failed to open process handle for PID {}", pid));
                            None
                        } else {
                            if let Err(e) = job.add_process(process_handle) {
                                sink.err(format!("[Warning] Failed to assign Java to Job Object: {}", e));
                                windows_sys::Win32::Foundation::CloseHandle(process_handle);
                                None
                            } else {
                                sink.log("[Security] Process attached to Job Object.");
                                windows_sys::Win32::Foundation::CloseHandle(process_handle);
                                Some(job)
                            }
                        }
                    }
                } else {
                    sink.err("[Warning] Could not get child process PID");
                    None
                }
            }
            Err(e) => {
                sink.err(format!("[Warning] Could not create Job Object: {}", e));
                None
            }
        }
    };

    // ── Wait / stop ──────────────────────────────────────────────────────────
    let exit_code: Option<i32> = tokio::select! {
        _ = &mut stop_rx => {
            sink.log("\n[RusTale] Stop signal received. Waiting for server to exit cleanly (up to 60s)...");
            match tokio::time::timeout(std::time::Duration::from_secs(60), child.wait()).await {
                Ok(Ok(status)) => {
                    sink.log(format!("[RusTale] Server closed cleanly with code: {:?}", status));
                    status.code()
                }
                Ok(Err(e)) => {
                    sink.err(format!("[RusTale] Error waiting for server: {}", e));
                    None
                }
                Err(_) => {
                    sink.err("[RusTale] The server took too long to save. FORCING CLOSE.");
                    let _ = child.kill().await;
                    None
                }
            }
        }
        status = child.wait() => {
            match status {
                Ok(s) => {
                    sink.log(format!("[RusTale] Server process terminated: {:?}", s));
                    s.code()
                }
                Err(e) => {
                    sink.err(format!("[RusTale] Error waiting for server process: {}", e));
                    None
                }
            }
        }
    };

    Ok(exit_code)
}
