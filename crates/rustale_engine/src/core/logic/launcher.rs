use crate::core::signals::FromCore;
use crate::core::errors::CoreError;
use crate::game::{GamePaths, LauncherStatus, progress::ProgressPayload};
use anyhow;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use crate::core::logic::checks::PreLaunchChecks;
use rustale_shared::config;

/// Custom error type for launch flow failures
#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("System requirements check failed: {0}")]
    SystemRequirements(String),
    
    #[error("Installation failed: {0}")]
    Installation(String),
    
    #[error("Game process failed to start: {0}")]
    ProcessStart(String),
    
    #[error("Launch cancelled by user")]
    Cancelled,
    
    #[error("Generic launch error: {0}")]
    Generic(String),
}

impl From<anyhow::Error> for LaunchError {
    fn from(err: anyhow::Error) -> Self {
        LaunchError::Generic(err.to_string())
    }
}

/// Internal launch flow that returns Result for error handling
async fn launch_flow_internal(
    tx: mpsc::Sender<FromCore>,
    internal_tx: mpsc::Sender<super::super::coordinator::CoordinatorEvent>,
    settings: rustale_shared::config::GameSettings,
    profile_name: String,
    profile_uuid: uuid::Uuid,
    version_hint: Option<i32>,
    cancel_token: Arc<AtomicBool>,
    client: rustale_shared::reqwest::Client,
) -> Result<(), LaunchError> {
    let base_dir = crate::system::get_app_dir();
    let loc = crate::lang::Localization::new();
    let base_dir_clone = base_dir.clone();

    // FIX: Wrap the base reporter in a proxy that emits semantic LauncherStatus changes
    // based on the phase key, so the UI can display specific text instead of generic "Busy".
    let tx_clone_for_status = tx.clone();
    let reporter_base = create_progress_reporter(tx.clone());
    let reporter = move |payload: crate::game::progress::ProgressPayload| {
        // FIX: Use stats presence as the primary download signal.
        // The legacy ProgressCallback sends raw text as the status key (e.g. "Downloading patch 0→123...")
        // so string matching on the key is fragile and case-sensitive.
        // DownloadStats are ONLY populated when bytes are actively transferring,
        // making stats.is_some() a reliable, key-agnostic download detector.
        if payload.stats.is_some() {
            let _ = tx_clone_for_status.try_send(FromCore::StatusChanged(LauncherStatus::Downloading));
        } else {
            // No active transfer → classify by message key (patching, extracting, etc.)
            let key_lower = payload.message_key.to_lowercase();
            if key_lower.contains("patch")
                || key_lower.contains("extract")
                || key_lower.contains("install")
                || key_lower.contains("migrat")
            {
                let _ = tx_clone_for_status.try_send(FromCore::StatusChanged(LauncherStatus::Migrating));
            }
        }
        // Always forward the full payload to the base reporter (ProgressUpdate signal).
        reporter_base(payload);
    };

    // [ROBUSTNESS] Phase 0: Pre-flight Checks
    let min_memory = settings.min_memory;
    let check_res: Result<Result<(), anyhow::Error>, tokio::task::JoinError> = tokio::task::spawn_blocking(move || {
        PreLaunchChecks::validate_system_requirements(
            min_memory, 
            &base_dir_clone
        )
    }).await;
    
    let check_res = check_res.map_err(|e| LaunchError::Generic(format!("Task join error: {}", e)))?;

    if let Err(e) = check_res {
        return Err(LaunchError::SystemRequirements(e.to_string()));
    }

    // --- PHASE 1: INSTALLATION / VERIFICATION ---
    let result = crate::game::patch_api::PatchApiFrontend::get_instance()
        .ensure_installed_with_weighted_progress(
            &base_dir,
            &settings.channel,
            if settings.game_version > 0 {
                Some(settings.game_version as i32)
            } else {
                version_hint
            },
            crate::game::install::InstallPolicy::NetworkUpdate,
            reporter,
            Some(cancel_token.clone()),
            &loc,
        )
        .await;

    if let Err(e) = result {
        let error_msg = e.to_string();
        if error_msg.contains("cancelled")
            || error_msg.contains("Cancelled")
            || error_msg.contains("cancel") {
            return Err(LaunchError::Cancelled);
        } else {
            return Err(LaunchError::Installation(format!("Installation failed: {}", e)));
        }
    }

    // [ROBUSTNESS] Cancellation check between Phase 1 and Phase 2+:
    // If the user cancelled while installation was in progress, the token is now set.
    // Stop here before starting the auth server, proxy, or game process.
    if cancel_token.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(LaunchError::Cancelled);
    }

    // --- PHASE 2: PATH PREPARATION ---
    let paths = GamePaths::new(base_dir.clone());
    // When game_version == 0 the user wants "latest". We must use the string "latest"
    // as the directory name — NOT the resolved numeric version — otherwise
    // version_dir() creates a spurious numbered folder (e.g. release/9/) alongside
    // the correct release/latest/ folder.
    let version_str = if settings.game_version > 0 {
        settings.game_version.to_string()
    } else {
        "latest".to_string()
    };

    let executable_path = paths.client_exe(&settings.channel, &version_str);
    let game_working_dir = paths.version_dir(&settings.channel, &version_str);
    let user_data_dir = paths.user_data();

    // Java Executable
    let java_real_path = paths.java_exec();
    let mut java_exec_for_client = java_real_path.to_string_lossy().to_string();

    // --- PROXY SETUP (HIJACK MODE) ---
    if settings.enable_online_fix {
        println!("[Core] Enabling Online Fix (Hijack Mode)...");
        match crate::java::proxy::setup_java_proxy(&java_real_path) {
            Ok(p) => {
                java_exec_for_client = p.to_string_lossy().to_string();
            }
            Err(e) => {
                eprintln!("[Core] Failed to setup Java Proxy: {}", e);
            }
        }
    } else {
        println!("[Core] Online Fix Disabled. Ensuring vanilla state...");
        if let Err(e) = crate::java::proxy::remove_java_proxy(&java_real_path) {
            eprintln!("[Core] Failed to remove Java Proxy: {}", e);
        }
    }

    // --- PHASE 3: AUTH SERVER AND ONLINE FIX CONFIG ---
    let mut auth_url = String::new();
    let mut auth_mode = "offline".to_string();
    let mut aurora_env_value = "local".to_string();
    let mut server_stop_tx = None;

    let mut server_port = crate::util::get_saved_port();
    if !auth_server::is_server_alive(server_port).await {
        server_port = crate::util::find_free_port();
    }

    if settings.enable_online_fix {
        let identity_dir = rustale_shared::config::get_identity_dir();
        let _ = auth_server::crypto::set_identity_dir(identity_dir.clone());
        auth_mode = "authenticated".to_string();
        crate::util::save_active_port(server_port);

        match settings.online_fix_mode {
            rustale_shared::config::OnlineFixMode::Local => {
                aurora_env_value = "local".to_string();
                auth_url = format!("http://127.0.0.000001:{}", server_port);

                let (stop_tx, stop_rx) = oneshot::channel();
                server_stop_tx = Some(stop_tx);

                let server_username = profile_name.clone();
                let server_uuid = profile_uuid.to_string();
                let server_game_dir = game_working_dir.clone();

                if !auth_server::is_server_alive(server_port).await {
                    println!("[Core] Starting Auth Server on port {}", server_port);
                    tokio::spawn(async move {
                        let _ = auth_server::start_server(
                            server_username,
                            server_uuid,
                            server_game_dir,
                            identity_dir,
                            stop_rx,
                            server_port,
                        )
                        .await;
                    });
                    // [ROBUSTNESS] Poll until ready instead of a fixed sleep.
                    // A fixed 500ms sleep causes false positives on fast machines
                    // (proceeds before the server is actually listening) and wastes
                    // time on slow machines. We poll up to 2 s (20 × 100 ms).
                    let mut server_ready = false;
                    for _ in 0..20 {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        if auth_server::is_server_alive(server_port).await {
                            server_ready = true;
                            break;
                        }
                    }
                    if !server_ready {
                        eprintln!("[Core] Auth Server did not become ready within 2 s on port {}", server_port);
                        // Non-fatal: token fetch will fail → retry loop handles it.
                    }
                } else {
                    println!("[Core] Updating existing Auth Server...");
                    let client_sync = rustale_shared::HTTP_CLIENT.clone();
                    let base_url = format!("http://127.0.1:{}", server_port);

                    let _ = client_sync
                        .post(format!("{}/internal/update-path", base_url))
                        .json(&serde_json::json!({"game_dir": game_working_dir.to_string_lossy()}))
                        .send()
                        .await;

                    let _ = client_sync
                        .post(format!("{}/internal/update-identity", base_url))
                        .json(
                            &serde_json::json!({"username": server_username, "uuid": server_uuid}),
                        )
                        .send()
                        .await;
                }
            }
            rustale_shared::config::OnlineFixMode::Sanasol => {
                aurora_env_value = "sanasol".to_string();
                auth_url = "https://sessions.sanasol.ws".to_string();
            }
        }

        // aurora installation
        if let Err(e) = crate::game::aurora::ensure_aurora_installed() {
            eprintln!("[Core] Aurora Error: {}", e);
        }
    }

    // Windows Secur32.dll injection
    let security_guard = if settings.enable_online_fix {
        #[cfg(target_os = "windows")]
        {
            let tools_aurora_path = rustale_shared::config::get_app_dir()
                .join("tools")
                .join(format!("aurora{}", std::env::consts::DLL_SUFFIX));
            let dll_path = executable_path.parent().unwrap().join("Secur32.dll");
            if let Ok(_) = std::fs::copy(&tools_aurora_path, &dll_path) {
                Some(crate::game::aurora::FileCleanupGuard { path: dll_path })
            } else {
                None
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            None
        }
    } else {
        None
    };

    // --- PHASE 4: TOKEN OBTENTION ---
    let mut auth_args = Vec::new();
    auth_args.push("--auth-mode".to_string());
    auth_args.push(auth_mode.clone());

    if auth_mode == "authenticated" {
        // Sync JWKS
        if let Ok(jwks) = crate::game::auth::fetch_remote_jwks(&client, &auth_url).await {
            auth_server::crypto::update_jwks_from_remote(jwks);
        }

        let mut tokens_res = Err(anyhow::anyhow!("Initial state"));
        for _ in 0..5 {
            match crate::game::auth::fetch_remote_tokens(
                &client,
                &auth_url,
                &profile_name,
                &profile_uuid.to_string(),
            )
            .await
            {
                Ok(t) => {
                    tokens_res = Ok(t);
                    break;
                }
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(500)).await,
            }
        }

        match tokens_res {
            Ok(tokens) => {
                auth_args.extend(vec![
                    "--identity-token".to_string(),
                    tokens.identity_token,
                    "--session-token".to_string(),
                    tokens.session_token,
                ]);
            }
            Err(e) => {
                // [ROBUSTNESS] All 5 retries failed — do NOT silently launch without tokens.
                // The game would start in "authenticated" mode but receive an auth rejection
                // from the server, producing a confusing in-game error instead of a clear
                // launcher message. Fail fast here so the user gets actionable feedback.
                return Err(LaunchError::Generic(format!(
                    "Failed to obtain auth tokens after 5 attempts: {}. \
                     Ensure the auth server is reachable and try again.",
                    e
                )));
            }
        }
    }

    // --- PHASE 5: MOD SYNC ---
    {
        let version_mods_src = paths.mods_dir(&settings.channel, &version_str);
        let global_mods_target = user_data_dir.join("Mods");
        if global_mods_target.exists() {
            let _ = tokio::fs::remove_dir_all(&global_mods_target).await;
        }
        let _ = tokio::fs::create_dir_all(&global_mods_target).await;
        if version_mods_src.exists() {
            if let Ok(mut entries) = tokio::fs::read_dir(&version_mods_src).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let path = entry.path();
                    if path.is_file() {
                        let filename = path.file_name().unwrap().to_string_lossy();
                        if filename.ends_with(".jar") {
                            let _ = tokio::fs::copy(&path, &global_mods_target.join(&*filename)).await;
                        }
                    }
                }
            }
        }
    }

    // --- PHASE 6: LAUNCH ---
    let launch_context = crate::game::launch::LaunchContext {
        player_name: profile_name,
        player_uuid: profile_uuid.to_string(),
        exec_path: executable_path,
        working_dir: game_working_dir,
        user_data_dir,
        java_path: java_exec_for_client,
        auth_args,
        env_vars: {
                    let mut env_vars = std::collections::HashMap::new();
                    // AURORA_ENV: leído por Aurora (la librería/agente inyectada en el juego)
                    env_vars.insert("AURORA_ENV".to_string(), aurora_env_value.clone());
                    // AURORA_MODE: leído por el proxy Java (binario del launcher renombrado como java).
                    // El proxy detecta su modo via esta variable en cli::run_proxy().
                    // FIX: sin esta variable, el proxy defaulteaba siempre a OnlineFixMode::Local
                    // aunque el usuario hubiese cambiado a Sanasol, causando que HYTALE_AUTH_DOMAIN
                    // se configurara con la URL local (127.0.0.000001:puerto) en lugar de la de Sanasol.
                    env_vars.insert("AURORA_MODE".to_string(), aurora_env_value);

                    if settings.enable_online_fix {
                        env_vars.insert("AURORA_URL".to_string(), auth_url);
                    }

            // Linux: inject Aurora via LD_PRELOAD so the .so is loaded into the
            // game process before any game code runs (equivalent to Windows Secur32.dll).
            #[cfg(target_os = "linux")]
            if settings.enable_online_fix {
                let aurora_path = base_dir
                    .join("tools")
                    .join(format!("aurora{}", std::env::consts::DLL_SUFFIX));

                if aurora_path.exists() {
                    // Prepend to any existing LD_PRELOAD the user/system may already have set.
                    let current = std::env::var("LD_PRELOAD").unwrap_or_default();
                    let aurora_str = aurora_path.to_string_lossy().to_string();
                    let new_preload = if current.is_empty() {
                        aurora_str
                    } else {
                        format!("{}:{}", aurora_str, current)
                    };
                    env_vars.insert("LD_PRELOAD".to_string(), new_preload);
                    println!("[Linux] Aurora injected via LD_PRELOAD: {:?}", aurora_path);
                } else {
                    eprintln!(
                        "[Linux] Aurora binary not found at {:?} — LD_PRELOAD not set",
                        aurora_path
                    );
                }
            }

            env_vars
        },
        jvm_args: if settings.java_args.is_empty() {
            None
        } else {
            Some(settings.java_args.split_whitespace().map(|s| s.to_string()).collect())
        },
    };

    match crate::game::launch::launch_game_with_async_agent(launch_context, client) {
        Ok(child) => {
            let _ = internal_tx
                .send(super::super::coordinator::CoordinatorEvent::GameProcessReady(
                    child,
                    server_stop_tx,
                    security_guard, // ¡Pasamos el guardián para que viva en el estado global!
                ))
                .await;
        }
        Err(e) => {
            return Err(LaunchError::ProcessStart(format!("Failed to start game process: {}", e)));
        }
    }

    Ok(())
}

/// Main launch flow that handles the complete game launch process
pub async fn launch_flow(
    tx: mpsc::Sender<FromCore>,
    internal_tx: mpsc::Sender<super::super::coordinator::CoordinatorEvent>,
    settings: rustale_shared::config::GameSettings,
    profile_name: String,
    profile_uuid: uuid::Uuid,
    version_hint: Option<i32>,
    cancel_token: Arc<AtomicBool>,
    client: rustale_shared::reqwest::Client,
) {
    // Run the internal flow with Result handling
    let result = launch_flow_internal(
        tx.clone(),
        internal_tx.clone(),
        settings,
        profile_name,
        profile_uuid,
        version_hint,
        cancel_token,
        client,
    ).await;

    match result {
        Ok(_) => {
            // Success is handled by Coordinator receiving GameProcessReady
            println!("[Launcher] Launch flow completed successfully");
        }
        Err(e) => {
            eprintln!("[Launcher] Launch flow failed: {:?}", e);
            
            // GUARANTEE: UI gets reset to Ready state on any error
            match e {
                LaunchError::Cancelled => {
                    let _ = tx.send(FromCore::StatusChanged(LauncherStatus::Ready)).await;
                }
                LaunchError::SystemRequirements(msg) => {
                    let _ = tx.send(FromCore::Error { 
                        message: format!("System Requirement Error: {}", msg), 
                        fatal: false 
                    }).await;
                    let _ = tx.send(FromCore::StatusChanged(LauncherStatus::Ready)).await;
                }
                LaunchError::Installation(msg) => {
                    let _ = internal_tx
                        .send(super::super::coordinator::CoordinatorEvent::LaunchFailed(
                            CoreError::LaunchError(msg)
                        ))
                        .await;
                }
                LaunchError::ProcessStart(msg) => {
                    let _ = internal_tx
                        .send(super::super::coordinator::CoordinatorEvent::LaunchFailed(
                            CoreError::LaunchError(msg)
                        ))
                        .await;
                }
                LaunchError::Generic(msg) => {
                    let _ = tx.send(FromCore::Error { 
                        message: format!("Launch Error: {}", msg), 
                        fatal: false 
                    }).await;
                    let _ = tx.send(FromCore::StatusChanged(LauncherStatus::Ready)).await;
                }
            }
        }
    }
}

/// Creates a progress reporter for installation operations
pub fn create_progress_reporter(tx: mpsc::Sender<FromCore>) -> impl Fn(ProgressPayload) + Send + Sync + 'static {
    move |payload| {
        let stats = payload.stats.map(|s| {
            format!(
                "{} - ETA: {}",
                s.speed_str,
                s.eta_str.unwrap_or_else(|| "Unknown".to_string())
            )
        });
        let _ = tx.try_send(FromCore::ProgressUpdate {
            phase: payload.message_key,
            msg_args: payload.message_args,
            progress: payload.global_progress,
            step_progress: payload.step_progress,
            current_step: payload.current_step,
            total_steps: payload.total_steps,
            stats,
        });
    }
}
