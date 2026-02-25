use super::errors::CoreError;
use super::handlers::profile_handler;
use super::signals::{FromCore, ToCore};
use super::state::{LogicState, TaskType};
use crate::core::logic::{launcher, mods_loader, profiles};
use crate::game::{self, GamePaths, LauncherStatus};
use crate::system;
use anyhow;
use rustale_shared::java::tracking::{
    cleanup_old_pids, clear_all_tracked_pids, get_tracked_pids, is_tracked_process, untrack_process,
};
use rustale_shared::profiles::ProfilesConfig;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

/// Cierra procesos Java relacionados con RusTale (solo en singleplayer)
async fn kill_java_processes() -> anyhow::Result<()> {
    // Solo cerrar procesos Java si NO somos un servidor dedicado
    if std::env::var("RUSTALE_IS_SERVER").is_ok() {
        println!("[Core] Skipping Java process cleanup for dedicated server");
        return Ok(());
    }

    println!("[Core] Cleaning up tracked Java processes...");

    // Limpiar PIDs antiguos primero
    cleanup_old_pids();

    // Obtener lista de PIDs trackeados
    let tracked_pids = get_tracked_pids();

    if tracked_pids.is_empty() {
        println!("[Core] No tracked Java processes to clean up");
        return Ok(());
    }

    println!(
        "[Core] Found {} tracked PIDs to check: {:?}",
        tracked_pids.len(),
        tracked_pids
    );

    for pid in tracked_pids {
        // Validar que el PID aún pertenece a un proceso Java de RusTale
        if is_tracked_process(pid) {
            println!("[Core] Killing tracked Java process PID: {}", pid);

            if cfg!(windows) {
                let _ = tokio::process::Command::new("taskkill")
                    .args(["/F", "/PID", &pid.to_string()])
                    .output()
                    .await;
            } else {
                let _ = tokio::process::Command::new("kill")
                    .args(["-9", &pid.to_string()])
                    .output()
                    .await;
            }
        } else {
            println!(
                "[Core] PID {} no longer valid or not a Java process, skipping",
                pid
            );
        }
    }

    // Limpiar tracker después de intentar matar
    clear_all_tracked_pids();

    Ok(())
}

pub enum CoordinatorEvent {
    GameProcessReady(
        tokio::process::Child,
        Option<oneshot::Sender<()>>,
        Option<crate::game::aurora::FileCleanupGuard>,
    ),
    LaunchFailed(CoreError),
}

pub async fn run(
    mut rx: mpsc::Receiver<ToCore>,
    tx: mpsc::Sender<FromCore>,
    initial_profiles: Option<ProfilesConfig>,
) {
    let mut state = if let Some(profiles) = initial_profiles {
        LogicState::with_profiles(profiles)
    } else {
        LogicState::new()
    };

    // Internal channel for spawned tasks to talk back to the main loop safely
    let (internal_tx, mut internal_rx) = mpsc::channel::<CoordinatorEvent>(32);

    // Configure a cleanup interval
    let mut cleanup_interval = tokio::time::interval(std::time::Duration::from_millis(500));

    loop {
        tokio::select! {
            // PERIODIC CLEANUP IS MANDATORY
            _ = cleanup_interval.tick() => {
                let before_count = state.active_task_count();
                let before_tasks = state.active_tasks();
                state.cleanup_finished_tasks();
                let after_count = state.active_task_count();
                let after_tasks = state.active_tasks();

                // Log task statistics for monitoring
                if before_count > 0 || after_count > 0 {
                    let stats = state.task_stats();
                    println!("[Core] Task Stats: {} active ({} total), {} long-running",
                            after_count, stats.total_tasks, stats.long_running_tasks);

                    if !before_tasks.is_empty() || !after_tasks.is_empty() {
                        println!("[Core] Active tasks before: {:?}, after: {:?}", before_tasks, after_tasks);
                    }
                }
            }

            // 1. External Command
            msg = rx.recv() => {
                match msg {
                    Some(cmd) => handle_ui_message(cmd, &mut state, &tx, &internal_tx).await,
                    None => break, // Channel closed
                }
            }

            // 2. Internal Async Results
            evt = internal_rx.recv() => {
                match evt {
                    Some(event) => handle_internal_event(event, &mut state, &tx).await,
                    // If all internal senders drop (panics), this stream closes.
                    // We must NOT exit the loop, just log warning.
                    None => {
                         eprintln!("[Core] Warning: Internal channel closed. Ensuring clean state.");
                         state.game_process = None; // Reset heavy flags
                    }
                }
            }

            // 3. Watchdog: Monitor running game process
            exit_status = async {
                if let Some(child) = &mut state.game_process {
                    child.wait().await
                } else {
                    std::future::pending().await
                }
            } => {
                match exit_status {
                    Ok(status) => {
                        println!("[Core] Game process exited with status: {}", status);
                    }
                    Err(e) => {
                        eprintln!("[Core] Game process error: {}", e);
                    }
                }

                // Remover el PID del tracker cuando el juego termina normalmente
                if let Some(child) = &state.game_process {
                    if let Some(pid) = child.id() {
                        untrack_process(pid);
                    }
                }

                state.game_process = None;
                state.security_guard = None; // ¡Aquí se borra la DLL cuando el juego se cierra!

                // Stop the auth server if it was running
                if let Some(stop_tx) = state.auth_server_stop.take() {
                    let _ = stop_tx.send(());
                }

                let _ = tx.send(FromCore::GameStopped).await;
                let _ = tx.send(FromCore::StatusChanged(LauncherStatus::Ready)).await;
                continue;
            }
        }
    }
}

pub struct ModReporter {
    pub tx: mpsc::Sender<FromCore>,
}

impl crate::core::logic::mods_loader::ModProgressReporter for ModReporter {
    fn on_progress(&self, phase: String, progress: f32, stats: Option<String>) {
        let _ = self.tx.try_send(FromCore::ProgressUpdate {
            phase,
            progress,
            step_progress: progress,
            current_step: 1,
            total_steps: 1,
            msg_args: vec![],
            stats,
        });
    }

    fn on_error(&self, error: String) {
        let _ = self.tx.try_send(FromCore::Error {
            message: error,
            fatal: false,
        });
    }

    fn on_finished(&self, _result_path: String) {}
}

// Handler for Async Results arriving from Spawned Tasks
async fn handle_internal_event(
    event: CoordinatorEvent,
    state: &mut LogicState,
    tx: &mpsc::Sender<FromCore>,
) {
    match event {
        CoordinatorEvent::GameProcessReady(mut child, auth_stop, sec_guard) => {
            // FIX (stale GameProcessReady after StopGame): If StopGame was sent while
            // the launch task was still in-flight, the spawned child must be killed
            // immediately instead of being registered as the "current" game process.
            if state.stop_requested {
                println!(
                    "[Core] Discarding stale GameProcessReady: stop was requested mid-launch, killing child."
                );
                let _ = child.kill().await;
                if let Some(s) = auth_stop {
                    let _ = s.send(());
                }
                state.stop_requested = false;
                state.tasks.remove(&TaskType::GameLaunch);
                // Status was already set to Ready by StopGame handler; nothing more to do.
                return;
            }

            state.game_process = Some(child);
            state.auth_server_stop = auth_stop;
            state.security_guard = sec_guard;
            // Clean specific task
            state.tasks.remove(&TaskType::GameLaunch);

            let _ = tx.send(FromCore::GameStarted).await;
            let _ = tx
                .send(FromCore::StatusChanged(LauncherStatus::Playing))
                .await;
        }

        CoordinatorEvent::LaunchFailed(error) => {
            if let Some(stop_tx) = state.auth_server_stop.take() {
                let _ = stop_tx.send(());
            }
            state.tasks.remove(&TaskType::GameLaunch);

            let _ = tx
                .send(FromCore::Error {
                    message: error.to_string(),
                    fatal: false,
                })
                .await;
            let _ = tx
                .send(FromCore::StatusChanged(LauncherStatus::Ready))
                .await;
        }
    }
}

async fn handle_ui_message(
    msg: ToCore,
    state: &mut LogicState,
    tx: &mpsc::Sender<FromCore>,
    internal_tx: &mpsc::Sender<CoordinatorEvent>,
) {
    match msg {
        ToCore::BootstrapSystem => {
            // 1. Filesystem init & Maintenance
            if let Err(e) = crate::system::lifecycle::bootstrap().await {
                let _ = tx.send(FromCore::BootstrapFailed(e.to_string())).await;
                return;
            }

            // 2. Load Data (Engine is the owner)
            let mut profiles = crate::system::load_profiles().await;
            let settings = crate::system::load_settings().await;

            // 3. Initialize State & Ensure Integrity
            let mut manager = crate::core::logic::profiles::ProfileManager::new(profiles.clone());
            if manager.ensure_integrity() {
                println!("[Core] Profiles healed during bootstrap.");
                let _ = manager.save_profiles().await;
                profiles = manager.get_config();
            }
            state.profile_manager = Some(manager);
            state.update_settings(settings.clone());

            // 4. Send State to UI
            let _ = tx
                .send(FromCore::BootstrapCompleted {
                    settings,
                    profiles: profiles,
                })
                .await;
        }
        ToCore::StartLogicLoop => {
            let _ = tx.send(FromCore::ReadyToDisplay).await;
        }
        ToCore::RequestInitialStatus(settings) => {
            println!("[Core] Starting RequestInitialStatus flow");
            state.update_settings(settings.clone());

            // Initialize services if needed (Lazy Loading pattern)
            if state.version_service.is_none() {
                println!("[Core] Initializing version service");
                state.version_service =
                    Some(crate::core::services::version_service::VersionService::new(
                        system::get_app_dir(),
                    ));
            }
            let version_svc = state.version_service.as_ref().unwrap();
            println!("[Core] Version service initialized");

            // 1. Trigger Local Scan
            println!("[Core] Starting local version scan");
            let _ = version_svc.scan_local_versions(&settings.channel, tx).await;
            println!("[Core] Local version scan completed");

            // 2. Trigger Remote Fetch (Blocking logic flow prevents race condition in UI)
            let tx_clone = tx.clone();
            let settings_clone = settings.clone();
            let base_dir = system::get_app_dir();

            println!("[Core] Starting remote version fetch");
            // Spawn managed task for network IO
            state.spawn_managed(TaskType::GenericIO, move |_| async move {
                println!("[Core] Remote fetch task started");
                let paths = GamePaths::new(base_dir);
                let svc =
                    crate::core::services::version_service::VersionService::new(paths.root.clone());

                // Fetch Remote
                println!("[Core] Fetching remote versions");
                let latest_remote = match svc
                    .refresh_versions(&settings_clone.channel, &tx_clone)
                    .await
                {
                    Ok(v) => {
                        println!("[Core] Remote fetch completed successfully: {:?}", v);
                        Some(v)
                    }
                    Err(e) => {
                        println!("[Core] Remote fetch failed: {}. Assuming offline mode.", e);
                        // Notify UI about offline mode
                        let _ = tx_clone
                            .send(FromCore::Error {
                                message: format!("launcher.error.offline_mode"), // Localization key
                                fatal: false,
                            })
                            .await;
                        None
                    }
                };

                // Determine if we're in offline mode
                let is_offline = latest_remote.is_none();

                // 3. Calculate Status with new data
                println!(
                    "[Core] Calculating status with remote: {:?}, offline: {}",
                    latest_remote, is_offline
                );
                let (status, _) = game::status::calculate_status(
                    &settings_clone,
                    &paths,
                    latest_remote,
                    is_offline,
                )
                .await;

                println!("[Core] Status calculated: {:?}, sending to UI", status);
                let _ = tx_clone.send(FromCore::StatusChanged(status)).await;
                let _ = tx_clone.send(FromCore::ReadyToDisplay).await;
                println!("[Core] RequestInitialStatus flow completed");
                crate::util::scrub_heap();

                crate::util::trim_memory_with_level(crate::util::TrimLevel::Aggressive);
            });
        }
        ToCore::LaunchGame => {
            let localization = state.localization.clone();
            crate::core::services::launcher_service::LauncherService::launch(
                state,
                tx.clone(),
                internal_tx.clone(),
                &localization,
            )
            .await;
        }

        ToCore::RequestVersionCheck(channel) => {
            if state.version_service.is_none() {
                state.version_service =
                    Some(crate::core::services::version_service::VersionService::new(
                        system::get_app_dir(),
                    ));
            }
            let _version_svc = state.version_service.as_ref().unwrap();

            let tx_clone = tx.clone();
            // We can call it directly since it's just a fetch and broadcast
            // But usually network stuff should be in a task.
            // coordinator is async so technically it's fine for small things,
            // but refresh_versions is async anyway.

            let base_dir = system::get_app_dir();
            state.spawn_managed(TaskType::GenericIO, move |_| async move {
                let svc = crate::core::services::version_service::VersionService::new(base_dir);
                let _ = svc.refresh_versions(&channel, &tx_clone).await;
            });
        }
        ToCore::RequestRepairVersion(version) => {
            let tx_clone = tx.clone();
            state.spawn_managed(TaskType::GenericIO, move |cancel_token| {
                handle_repair(tx_clone, version, cancel_token)
            });
        }
        ToCore::RequestDeleteVersion(version) => {
            let base_dir = system::get_app_dir();
            // FIX: Was hardcoded to "stable", ignoring the user's configured channel
            // (e.g. "pre-release"). Now uses the authoritative channel stored in state.
            let channel = state.settings.channel.clone();
            let version_str = if version == 0 {
                "latest".to_string()
            } else {
                version.to_string()
            };
            let paths = GamePaths::new(base_dir);
            let version_dir = paths.version_dir(&channel, &version_str);
            let _ = tokio::fs::remove_dir_all(version_dir).await;
            // Notify UI to refresh
            let _ = tx
                .send(FromCore::StatusChanged(LauncherStatus::Ready))
                .await;
        }
        ToCore::LoadJavaInfo => {
            let tx = tx.clone();
            let base_dir = system::get_app_dir();
            state.spawn_managed(TaskType::GenericIO, move |_| async move {
                match crate::java::detection::ensure_java_available(&base_dir).await {
                    Ok(info) => {
                        let _ = tx.send(FromCore::JavaInfoLoaded(info)).await;
                    }
                    Err(e) => {
                        let _ = tx
                            .send(FromCore::OperationFailed {
                                error: CoreError::IOError(e.to_string()),
                            })
                            .await;
                    }
                }
            });
        }
        ToCore::MigrateData { from, to } => {
            let tx_clone = tx.clone();
            state.spawn_managed(TaskType::GenericIO, move |cancel_token| async move {
                // Check cancellation before expensive operation
                if cancel_token.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }

                let _ = tx_clone
                    .send(FromCore::StatusChanged(LauncherStatus::Busy))
                    .await;

                let tx_progress = tx_clone.clone();
                let dest = to.clone();
                let result = crate::util::move_dir_with_progress(from, to, move |pct| {
                    let _ = tx_progress.try_send(FromCore::ProgressUpdate {
                        phase: "Migrating files...".to_string(),
                        msg_args: vec![],
                        progress: pct / 100.0,
                        step_progress: pct / 100.0,
                        current_step: 1,
                        total_steps: 1,
                        stats: None,
                    });
                })
                .await;

                let _ = tx_clone
                    .send(FromCore::MigrationFinished(result.map(|_| dest).map_err(
                        |e: anyhow::Error| CoreError::GenericError(e.to_string()),
                    )))
                    .await;
            });
        }
        ToCore::AbortOperation => {
            // Log all active tasks before cancellation
            let active_tasks = state.active_tasks();
            if !active_tasks.is_empty() {
                println!(
                    "[Core] Aborting {} active tasks: {:?}",
                    active_tasks.len(),
                    active_tasks
                );
            }
            state.cancel_all();
            let _ = tx
                .send(FromCore::StatusChanged(LauncherStatus::Ready))
                .await;
        }

        ToCore::StopGame => {
            // Mark stop_requested so that a stale GameProcessReady arriving
            // after cancellation is discarded rather than registered.
            state.stop_requested = true;

            if let Some(mut child) = state.game_process.take() {
                println!("[Core] StopGame: killing game process.");
                let _ = child.kill().await;
            } else {
                println!(
                    "[Core] StopGame: no running process (launch may be in-flight; stop_requested set)."
                );
            }

            // NUEVO: Cerrar procesos Java relacionados (solo en singleplayer)
            if let Err(e) = kill_java_processes().await {
                eprintln!("[Core] Error killing Java processes: {}", e);
            }

            if let Some(stop_tx) = state.auth_server_stop.take() {
                let _ = stop_tx.send(());
            }
            state.security_guard = None; // Limpiar DLL inyectada
            state.cancel_task(TaskType::GameLaunch);
            // If there was no running process (task was still in-flight), reset the flag now
            // so we don't leave it set for the next launch attempt.  The GameProcessReady
            // guard will clear it if a stale child arrives later.
            if !state.is_task_running(&TaskType::GameLaunch) {
                state.stop_requested = false;
            }
            let _ = tx.send(FromCore::GameStopped).await;
            let _ = tx
                .send(FromCore::StatusChanged(LauncherStatus::Ready))
                .await;
        }
        ToCore::TrimMemory => {
            crate::util::trim_memory();
        }
        ToCore::OpenGameFolder => {
            crate::util::open_game_folder();
        }
        ToCore::ExitApp => {
            std::process::exit(0);
        }
        ToCore::CheckForLauncherUpdates => {
            // Check if app update is already running
            if state.is_task_running(&TaskType::AppUpdate) {
                println!("[Core] App update check already in progress, ignoring request");
                return;
            }

            // The provided snippet for LAST_ACTIVITY.lock() was incomplete and syntactically incorrect.
            // Assuming the intent was to add some rate-limiting or activity tracking,
            // but without the full context or definition of LAST_ACTIVITY and Instant,
            // and to maintain syntactic correctness, this part is omitted.
            // The original `state.spawn_managed` call is kept.

            let client = state.http_client.clone();
            let tx_clone = tx.clone();
            state.spawn_managed(TaskType::AppUpdate, move |_cancel_token| async move {
                let res = crate::core::updater::check_for_updates(&client).await;
                match res {
                    Ok(info) => {
                        let _ = tx_clone
                            .send(FromCore::LauncherUpdateCheckResult(Ok(info)))
                            .await;
                    }
                    Err(e) => {
                        let _ = tx_clone
                            .send(FromCore::LauncherUpdateCheckResult(Err(
                                CoreError::NetworkError(e.to_string()),
                            )))
                            .await;
                    }
                }
            });
        }
        ToCore::PerformLauncherUpdate(url) => {
            // Check if app update is already running
            if state.is_task_running(&TaskType::AppUpdate) {
                println!("[Core] App update already in progress, ignoring request");
                let _ = tx
                    .send(FromCore::Error {
                        message: "App update already in progress".to_string(),
                        fatal: false,
                    })
                    .await;
                return;
            }

            let _ = tx.send(FromCore::StatusChanged(LauncherStatus::Busy)).await;
            let client = state.download_client.clone();
            let tx_clone = tx.clone();

            state.spawn_managed(TaskType::AppUpdate, move |_cancel_token| async move {
                let _ = tx_clone
                    .send(FromCore::LauncherUpdateProgress(
                        0.0,
                        "Downloading update...".to_string(),
                    ))
                    .await;

                let res = crate::core::updater::perform_update(client, url).await;
                match res {
                    Ok(_) => {
                        let _ = tx_clone.send(FromCore::LauncherUpdateFinished).await;
                    }
                    Err(e) => {
                        let _ = tx_clone
                            .send(FromCore::OperationFailed {
                                error: CoreError::NetworkError(format!("Update failed: {}", e)),
                            })
                            .await;
                        let _ = tx_clone
                            .send(FromCore::StatusChanged(LauncherStatus::Ready))
                            .await;
                    }
                }
            });
        }
        ToCore::SearchMods {
            query,
            offset,
            limit,
        } => {
            // Check if mod search is already running
            if state.is_task_running(&TaskType::ModSearch) {
                println!("[Core] Mod search already in progress, ignoring request");
                return;
            }

            let client = state.http_client.clone();
            let tx = tx.clone();
            state.spawn_managed(TaskType::ModSearch, move |_cancel_token| async move {
                // Use the optimized ModsService for searching
                let mods_service = mods_loader::ModsService::new(client.clone());
                let res = mods_service.search(&query, offset, limit).await;
                let _ = tx
                    .send(FromCore::ModsSearchLoaded(
                        res.map_err(|e| CoreError::GenericError(e.to_string())),
                    ))
                    .await;
            });
        }
        ToCore::LoadLocalMods { channel, version } => {
            let tx = tx.clone();
            let base_dir = system::get_app_dir();
            let client = state.download_client.clone();

            // Generate a task key to prevent duplicate loading
            // We use GenericIO as a general bucket for this I/O task
            let task_id = TaskType::GenericIO;

            state.spawn_managed(task_id, move |cancel_token| async move {
                // Check token before expensive blocking op
                if cancel_token.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }

                let mods_service = mods_loader::ModsService::new(client.clone());

                // Perform load...
                let res = mods_service
                    .load_local_mods(base_dir.clone(), channel.clone(), version.clone())
                    .await;

                // Ensure patch integrity verification happens safely
                if res.is_ok() {
                    let paths = crate::game::GamePaths::new(base_dir);
                    // Verify integrity only if not cancelled
                    if !cancel_token.load(std::sync::atomic::Ordering::Relaxed) {
                        if let Err(e) = crate::game::zip_mods::verify_patch_integrity(
                            &paths, &channel, &version,
                        ) {
                            eprintln!("[Core] Warning: Patch integrity verification failed: {}", e);
                        }
                    }
                }

                let _ = tx
                    .send(FromCore::LocalModsLoaded(
                        res.map_err(|e| CoreError::GenericError(e.to_string())),
                    ))
                    .await;
            });
        }
        ToCore::InstallMod(req) => {
            // Check if mod installation is already running for this mod
            if state.is_task_running(&TaskType::ModInstallation(req.mod_id.clone())) {
                println!(
                    "[Core] Mod installation already in progress for: {}",
                    req.mod_id
                );
                let _ = tx
                    .send(FromCore::Error {
                        message: format!(
                            "Mod installation already in progress for: {}",
                            req.mod_id
                        ),
                        fatal: false,
                    })
                    .await;
                return;
            }

            let tx = tx.clone();
            let base_dir = system::get_app_dir();
            let client = state.download_client.clone();

            // Use the optimized ModsService for centralized installation
            let mods_service = mods_loader::ModsService::new(client.clone());

            // Use managed spawn with cancellation support
            state.spawn_managed(
                TaskType::ModInstallation(req.mod_id.clone()),
                move |cancel_token| async move {
                    let settings = system::load_settings().await;
                    let reporter = Arc::new(ModReporter { tx: tx.clone() });
                    let res = mods_service
                        .install_mod(
                            req,
                            settings.channel.clone(),
                            settings.game_version,
                            base_dir,
                            reporter,
                            cancel_token,
                        )
                        .await;
                    let _ = tx
                        .send(FromCore::ModOperationFinished(
                            res.map(|_mod_id| ())
                                .map_err(|e| CoreError::GenericError(e.to_string())),
                        ))
                        .await;
                },
            );
        }
        ToCore::UninstallMod(mod_id) => {
            // Check if mod operation is already running for this mod
            if state.is_task_running(&TaskType::ModInstallation(mod_id.clone())) {
                println!("[Core] Mod operation already in progress for: {}", mod_id);
                let _ = tx
                    .send(FromCore::Error {
                        message: format!("Mod operation already in progress for: {}", mod_id),
                        fatal: false,
                    })
                    .await;
                return;
            }

            let tx = tx.clone();
            let base_dir = system::get_app_dir();
            let client = state.download_client.clone();

            state.spawn_managed(
                TaskType::ModInstallation(mod_id.clone()),
                move |cancel_token| async move {
                    let settings = system::load_settings().await;
                    // Use the optimized ModsService for centralized uninstallation
                    let mods_service = mods_loader::ModsService::new(client.clone());
                    let reporter = Arc::new(ModReporter { tx: tx.clone() });
                    let res = mods_service
                        .uninstall_mod(
                            mod_id.clone(),
                            settings.channel.clone(),
                            settings.game_version,
                            base_dir,
                            reporter,
                            cancel_token,
                        )
                        .await;
                    let _ = tx
                        .send(FromCore::ModOperationFinished(
                            res.map_err(|e| CoreError::GenericError(e.to_string())),
                        ))
                        .await;
                },
            );
        }
        ToCore::ToggleMod(name, enabled) => {
            // Check if mod operation is already running for this mod
            if state.is_task_running(&TaskType::ModInstallation(name.clone())) {
                println!("[Core] Mod toggle already in progress for: {}", name);
                let _ = tx
                    .send(FromCore::Error {
                        message: format!("Mod operation already in progress for: {}", name),
                        fatal: false,
                    })
                    .await;
                return;
            }

            let tx = tx.clone();
            let base_dir = system::get_app_dir();
            let client = state.download_client.clone();

            state.spawn_managed(
                TaskType::ModInstallation(name.clone()),
                move |cancel_token| async move {
                    // Check cancellation before expensive operation
                    if cancel_token.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }

                    let settings = system::load_settings().await;
                    // Use the optimized ModsService for centralized toggle operations
                    let mods_service = mods_loader::ModsService::new(client.clone());
                    let res = mods_service
                        .toggle_mod(
                            name,
                            enabled,
                            base_dir,
                            settings.channel,
                            if settings.game_version == 0 {
                                "latest".to_string()
                            } else {
                                settings.game_version.to_string()
                            },
                        )
                        .await;

                    // Check cancellation before sending result
                    if cancel_token.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }

                    // Signals refresh
                    let _ = tx
                        .send(FromCore::ModOperationFinished(
                            res.map_err(|e| CoreError::GenericError(e.to_string())),
                        ))
                        .await;
                },
            );
        }
        ToCore::ToggleZipPatch(id, enabled) => {
            // Check if patch operation is already running for this patch
            if state.is_task_running(&TaskType::ModInstallation(format!("patch_{}", id))) {
                println!("[Core] Patch toggle already in progress for: {}", id);
                let _ = tx
                    .send(FromCore::Error {
                        message: format!("Patch operation already in progress for: {}", id),
                        fatal: false,
                    })
                    .await;
                return;
            }

            let tx = tx.clone();
            let base_dir = system::get_app_dir();
            let client = state.download_client.clone();

            state.spawn_managed(
                TaskType::ModInstallation(format!("patch_{}", id)),
                move |cancel_token| async move {
                    // Check cancellation before expensive operation
                    if cancel_token.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }

                    let settings = system::load_settings().await;
                    // Use the optimized ModsService for centralized patch toggle operations
                    let mods_service = mods_loader::ModsService::new(client.clone());
                    let res = mods_service
                        .toggle_patch(
                            id,
                            enabled,
                            base_dir,
                            settings.channel,
                            if settings.game_version == 0 {
                                "latest".to_string()
                            } else {
                                settings.game_version.to_string()
                            },
                        )
                        .await;

                    // Check cancellation before sending result
                    if cancel_token.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }

                    let _ = tx
                        .send(FromCore::ModOperationFinished(
                            res.map_err(|e| CoreError::GenericError(e.to_string())),
                        ))
                        .await;
                },
            );
        }
        ToCore::InitializeProfiles(profiles) => {
            if let Some(manager) = &mut state.profile_manager {
                manager.update_profiles(profiles);
            } else {
                state.profile_manager = Some(profiles::ProfileManager::new(profiles));
            }
        }
        ToCore::SetCurrentProfile(id) => {
            profile_handler::handle_set_current_profile(state, tx, id).await;
        }
        ToCore::CreateProfile(name) => {
            profile_handler::handle_create_profile(state, tx, name).await;
        }
        ToCore::UpdateProfileName(id, name) => {
            profile_handler::handle_update_profile_name(state, tx, id, name).await;
        }
        ToCore::UpdateProfileUuid(old_id, new_id) => {
            if let Some(manager) = &mut state.profile_manager {
                if manager.update_profile_uuid(old_id, new_id).is_some() {
                    if let Err(e) = manager.save_profiles().await {
                        let _ = tx
                            .send(FromCore::Error {
                                message: format!("Failed to save profile UUID: {}", e),
                                fatal: false,
                            })
                            .await;
                    } else {
                        let _ = tx.send(FromCore::SettingsSaved).await;
                    }
                }
            }
        }
        ToCore::DeleteProfile(id) => {
            profile_handler::handle_delete_profile(state, tx, id).await;
        }
        ToCore::SaveSettings(settings) | ToCore::UpdateSettings(settings) => {
            state.update_settings(settings.clone());
            // Offload file I/O from Coordinator loop to avoid stalls.
            // Uses SettingsSave (NOT GenericIO) so it never collides with
            // version checks, mod loads, news fetches, etc.
            let tx_clone = tx.clone();
            state.spawn_managed(TaskType::SettingsSave, move |_| async move {
                match system::save_settings(&settings).await {
                    Ok(_) => {
                        let _ = tx_clone.send(FromCore::SettingsSaved).await;
                    }
                    Err(e) => {
                        let _ = tx_clone
                            .send(FromCore::Error {
                                message: format!("Failed to save settings: {}", e),
                                fatal: false,
                            })
                            .await;
                    }
                }
            });
        }
        ToCore::SaveProfile(profiles) => {
            profile_handler::handle_save_profile(state, tx, profiles).await;
        }
        ToCore::ImportProfile { path } => {
            if let Some(manager) = &mut state.profile_manager {
                let tx_clone = tx.clone();

                // Validar el archivo antes de importar usando la función utilitaria
                let validation_result = profiles::load_profile_file(&path).await;
                match validation_result {
                    Ok(_) => {
                        // Enviar progreso inicial
                        let _ = tx_clone
                            .send(FromCore::ProgressUpdate {
                                phase: "profile.importing".to_string(),
                                progress: 0.3,
                                step_progress: 0.3,
                                current_step: 1,
                                total_steps: 1,
                                msg_args: vec![],
                                stats: None,
                            })
                            .await;

                        // Usar el método centralizado que carga desde archivo e importa
                        match manager.import_profiles(&path).await {
                            Ok(count) => {
                                let _ = tx_clone
                                    .send(FromCore::ProgressUpdate {
                                        phase: "profile.importing".to_string(),
                                        progress: 0.8,
                                        step_progress: 0.8,
                                        current_step: 1,
                                        total_steps: 1,
                                        msg_args: vec![],
                                        stats: None,
                                    })
                                    .await;

                                // Guardar los cambios
                                if let Err(e) = manager.save_profiles().await {
                                    let _ = tx_clone
                                        .send(FromCore::Error {
                                            message: format!(
                                                "Failed to save imported profiles: {}",
                                                e
                                            ),
                                            fatal: false,
                                        })
                                        .await;
                                } else {
                                    let _ = tx_clone
                                        .send(FromCore::Error {
                                            message: format!(
                                                "Successfully imported {} profiles",
                                                count
                                            ),
                                            fatal: false,
                                        })
                                        .await;
                                    let _ = tx_clone.send(FromCore::SettingsSaved).await;
                                }
                            }
                            Err(e) => {
                                let _ = tx_clone
                                    .send(FromCore::Error {
                                        message: format!("Failed to import profiles: {}", e),
                                        fatal: false,
                                    })
                                    .await;
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx_clone
                            .send(FromCore::Error {
                                message: format!("Invalid profile file: {}", e),
                                fatal: false,
                            })
                            .await;
                    }
                }
            }
        }
        ToCore::ImportProfilesFromMemory { profiles } => {
            if let Some(manager) = &mut state.profile_manager {
                let tx_clone = tx.clone();

                // Enviar progreso inicial
                let _ = tx_clone
                    .send(FromCore::ProgressUpdate {
                        phase: "profile.importing".to_string(),
                        progress: 0.3,
                        step_progress: 0.3,
                        current_step: 1,
                        total_steps: 1,
                        msg_args: vec![],
                        stats: None,
                    })
                    .await;

                // Usar el método para importar desde vector (ya cargado en memoria)
                match manager.import_profiles_from_vec(&profiles) {
                    Ok(count) => {
                        let _ = tx_clone
                            .send(FromCore::ProgressUpdate {
                                phase: "profile.importing".to_string(),
                                progress: 0.8,
                                step_progress: 0.8,
                                current_step: 1,
                                total_steps: 1,
                                msg_args: vec![],
                                stats: None,
                            })
                            .await;

                        // Guardar los cambios
                        if let Err(e) = manager.save_profiles().await {
                            let _ = tx_clone
                                .send(FromCore::Error {
                                    message: format!("Failed to save imported profiles: {}", e),
                                    fatal: false,
                                })
                                .await;
                        } else {
                            let _ = tx_clone
                                .send(FromCore::Error {
                                    message: format!(
                                        "Successfully imported {} profiles from memory",
                                        count
                                    ),
                                    fatal: false,
                                })
                                .await;
                            let _ = tx_clone.send(FromCore::SettingsSaved).await;
                        }
                    }
                    Err(e) => {
                        let _ = tx_clone
                            .send(FromCore::Error {
                                message: format!("Failed to import profiles from memory: {}", e),
                                fatal: false,
                            })
                            .await;
                    }
                }
            }
        }
        ToCore::CheckForUpdates => {
            // Deduplication via TaskType
            if state.is_task_running(&TaskType::AppUpdate) {
                println!(
                    "[Core] App update check already running, skipping generic update check request"
                );
                return;
            }

            let tx = tx.clone();
            let base_dir = system::get_app_dir();
            let client = state.http_client.clone();

            // Reuse AppUpdate task type or ideally ModUpdateCheck if strictly separate
            // For now using AppUpdate as a shared "Update Check" slot
            state.spawn_managed(TaskType::AppUpdate, move |cancel_token| async move {
                let settings = system::load_settings().await;

                // Check cancellation point 1
                if cancel_token.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }

                // Use the optimized ModsService for update checking
                let mods_service = mods_loader::ModsService::new(client);

                // Load installed mods and patches
                let manifest = crate::game::mods::load_manifest(
                    &base_dir,
                    &settings.channel,
                    &if settings.game_version == 0 {
                        "latest".to_string()
                    } else {
                        settings.game_version.to_string()
                    },
                )
                .await;

                // Check cancellation point 2
                if cancel_token.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }

                let paths = crate::game::GamePaths::new(base_dir.clone());
                let patches = crate::game::zip_mods::list_patches(paths.core_patches_dir(
                    &settings.channel,
                    &if settings.game_version == 0 {
                        "latest".to_string()
                    } else {
                        settings.game_version.to_string()
                    },
                ))
                .unwrap_or_default();

                // Check for updates using the centralized service
                let version_str = if settings.game_version == 0 {
                    "latest".to_string()
                } else {
                    settings.game_version.to_string()
                };

                match mods_service
                    .check_updates(manifest, patches, &version_str)
                    .await
                {
                    Ok((updates, cached_map)) => {
                        let _ = tx
                            .send(FromCore::UpdatesLoaded(Ok((updates, cached_map))))
                            .await;
                    }
                    Err(e) => {
                        let _ = tx
                            .send(FromCore::UpdatesLoaded(Err(CoreError::GenericError(
                                e.to_string(),
                            ))))
                            .await;
                    }
                }
            });
        }
        ToCore::LoadVersions(mod_id) => {
            // Check if version loading is already running for this mod
            if state.is_task_running(&TaskType::ModInstallation(format!("versions_{}", mod_id))) {
                println!(
                    "[Core] Version loading already in progress for mod: {}",
                    mod_id
                );
                return;
            }

            let tx = tx.clone();
            let client = state.http_client.clone();
            state.spawn_managed(
                TaskType::ModInstallation(format!("versions_{}", mod_id)),
                move |cancel_token| async move {
                    // Check cancellation before expensive operation
                    if cancel_token.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }

                    // Use the optimized ModsService for version loading
                    let mods_service = mods_loader::ModsService::new(client);
                    match mods_service.get_versions(&mod_id).await {
                        Ok(versions) => {
                            // Check cancellation before sending result
                            if cancel_token.load(std::sync::atomic::Ordering::Relaxed) {
                                return;
                            }
                            let _ = tx
                                .send(FromCore::VersionsLoaded(Ok((mod_id, versions))))
                                .await;
                        }
                        Err(e) => {
                            // Check cancellation before sending result
                            if cancel_token.load(std::sync::atomic::Ordering::Relaxed) {
                                return;
                            }
                            let _ = tx
                                .send(FromCore::VersionsLoaded(Err(CoreError::GenericError(
                                    e.to_string(),
                                ))))
                                .await;
                        }
                    }
                },
            );
        }
        ToCore::GetCacheStats => {
            let tx = tx.clone();
            state.spawn_managed(TaskType::GenericIO, move |_| async move {
                let cache_stats = crate::game::patch_api::get_shared_cache()
                    .get_cache_stats()
                    .await;
                let _ = tx
                    .send(FromCore::CacheStatsLoaded(
                        cache_stats.map_err(|e| CoreError::GenericError(e.to_string())),
                    ))
                    .await;
            });
        }
        ToCore::FetchNews => {
            let tx = tx.clone();
            state.spawn_managed(TaskType::GenericIO, move |_| async move {
                match crate::news::fetch_news().await {
                    Ok(posts) => {
                        let _ = tx.send(FromCore::NewsLoaded(Ok(posts))).await;
                    }
                    Err(e) => {
                        let _ = tx
                            .send(FromCore::NewsLoaded(Err(CoreError::NetworkError(
                                e.to_string(),
                            ))))
                            .await;
                    }
                }
            });
        }
        ToCore::UseDataLocation { path } => {
            // Use existing data from a different location without moving
            let tx_clone = tx.clone();

            state.spawn_managed(TaskType::GenericIO, move |cancel_token| async move {
                // Check cancellation before expensive operation
                if cancel_token.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }

                let _ = tx_clone
                    .send(FromCore::StatusChanged(LauncherStatus::Busy))
                    .await;

                // Verify the target path exists and has valid data
                if !path.exists() {
                    let _ = tx_clone
                        .send(FromCore::Error {
                            message: format!("Path does not exist: {}", path.display()),
                            fatal: false,
                        })
                        .await;
                    let _ = tx_clone
                        .send(FromCore::StatusChanged(LauncherStatus::Ready))
                        .await;
                    return;
                }

                // Save the new path to bootstrap config for next launch
                if let Err(e) = crate::system::save_bootstrap_path(&path) {
                    let _ = tx_clone
                        .send(FromCore::Error {
                            message: format!("Failed to save data location: {}", e),
                            fatal: false,
                        })
                        .await;
                    let _ = tx_clone
                        .send(FromCore::StatusChanged(LauncherStatus::Ready))
                        .await;
                    return;
                }

                let _ = tx_clone
                    .send(FromCore::MigrationFinished(Ok(path.clone())))
                    .await;
                let _ = tx_clone
                    .send(FromCore::StatusChanged(LauncherStatus::Ready))
                    .await;
            });
        }
        ToCore::WatchdogCheck => {
            // Check game process status and system health
            let game_running = state.game_process.is_some();
            let active_task_count = state.active_task_count();

            // Report status back to UI
            let _ = tx
                .send(FromCore::StatusChanged(if game_running {
                    LauncherStatus::Playing
                } else if active_task_count > 0 {
                    LauncherStatus::Busy
                } else {
                    LauncherStatus::Ready
                }))
                .await;
        }
        _ => {}
    }
}

async fn handle_repair(
    tx: mpsc::Sender<FromCore>,
    version: u32,
    cancel_token: Arc<std::sync::atomic::AtomicBool>,
) {
    let _ = tx
        .send(FromCore::StatusChanged(LauncherStatus::Downloading))
        .await;

    let base_dir = system::get_app_dir();
    let loc = crate::lang::Localization::new();
    let reporter = launcher::create_progress_reporter(tx.clone());
    let _client = rustale_shared::HTTP_CLIENT.clone();

    let result = crate::game::patch_api::PatchApiFrontend::get_instance()
        .ensure_installed_with_weighted_progress(
            &base_dir,
            "stable", // Assuming stable or detect from settings
            Some(version as i32),
            crate::game::InstallPolicy::Repair,
            reporter,
            Some(cancel_token),
            &loc,
        )
        .await;

    let _ = tx
        .send(FromCore::RepairOperationFinished(
            result.map_err(|e| CoreError::GenericError(e.to_string())),
        ))
        .await;
    let _ = tx
        .send(FromCore::StatusChanged(LauncherStatus::Ready))
        .await;
}
