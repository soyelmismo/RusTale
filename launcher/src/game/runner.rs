use crate::Message;
use crate::config::GameSettings;
use crate::game::install::InstallPolicy;
use crate::game::progress::ProgressTracker;
use iced::advanced::subscription::{self, Hasher, Recipe};
use iced::futures::{SinkExt, StreamExt};
use iced::{Subscription, stream};
use std::hash::Hash;
use std::io::Write;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task;

/// Security guard to clean up temporary files when exiting the scope via Drop.
struct FileCleanupGuard {
    path: PathBuf,
}

impl Drop for FileCleanupGuard {
    fn drop(&mut self) {
        if self.path.exists() {
            // Try to delete the file. Use std::fs sync because Drop cannot be async.
            // Ignore errors (e.g: if the user already deleted it) to avoid panics on close.
            let _ = std::fs::remove_file(&self.path);
            println!("[Cleanup] Injected file deleted: {:?}", self.path);
        }
    }
}

// Include the binary compiled by build.rs
const AURORA_BIN: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/aurora_embed.bin"));

pub fn run(
    settings: GameSettings,
    player_name: String,
    player_uuid: String,
    client: reqwest::Client,
    target_version: Option<i32>,
    install_policy: InstallPolicy,
) -> Subscription<Message> {
    subscription::from_recipe(Runner {
        settings,
        player_name,
        player_uuid,
        client,
        target_version,
        install_policy,
    })
}

struct Runner {
    settings: GameSettings,
    player_name: String,
    player_uuid: String,
    client: reqwest::Client,
    target_version: Option<i32>,
    install_policy: InstallPolicy,
}

impl Recipe for Runner {
    type Output = Message;

    fn hash(&self, state: &mut Hasher) {
        std::any::TypeId::of::<Self>().hash(state);
        self.settings.hash(state);
        self.player_name.hash(state);
        self.player_uuid.hash(state);
        self.target_version.hash(state);
        self.install_policy.hash(state);
    }

    fn stream(
        self: Box<Self>,
        _input: subscription::EventStream,
    ) -> iced::futures::stream::BoxStream<'static, Self::Output> {
        let settings = self.settings;
        let player_name = self.player_name;
        let player_uuid = self.player_uuid;
        let client = self.client;
        let install_policy = self.install_policy;

        let s = stream::channel::<Message>(
            100,
            move |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
                let base_dir = crate::config::get_app_dir();

                // --- PHASE 1: VERIFICATION / INSTALLATION ---
                let (tx, mut rx) = mpsc::channel(100);
                let (res_tx, mut res_rx) = mpsc::channel(1);

                let tracker = ProgressTracker::new()
                    .add_step("check", 1.0)
                    .add_step("jre", 20.0)
                    .add_step("butler", 10.0)
                    .add_step("version", 4.0)
                    .add_step("download", 45.0)
                    .add_step("install", 15.0)
                    .add_step("cleanup", 1.0);
                let progress_calculator = tracker.clone();

                let install_settings = settings.clone();
                let install_base = base_dir.clone();
                let install_client = client.clone();

                tokio::spawn(async move {
                    let progress_tx = tx.clone();
                    let result = crate::game::ensure_installed(
                        &install_client,
                        &install_base,
                        &install_settings.channel,
                        if install_settings.game_version > 0 {
                            Some(install_settings.game_version as i32)
                        } else {
                            Some(0)
                        },
                        install_policy,
                        move |phase, sub_p, msg| {
                            let sub_p_f32 = sub_p as f32;
                            let general_p = progress_calculator.calculate(phase, sub_p_f32);
                            let _ = progress_tx.try_send(Message::DownloadProgress {
                                progress: general_p,
                                sub_progress: sub_p_f32,
                                speed: msg.to_string(),
                            });
                        },
                    )
                    .await;

                    if let Err(e) = &result {
                        let _ = tx.send(crate::Message::DownloadError(e.to_string())).await;
                    }
                    let _ = res_tx.send(result).await;
                });

                while let Some(msg) = rx.recv().await {
                    let _ = output.send(msg).await;
                }

                match res_rx.recv().await {
                    Some(Ok(_)) => {}
                    _ => {
                        let _ = output.send(Message::GameStopped).await;
                        return;
                    }
                }

                // --- PHASE 2: PATH PREPARATION ---
                let paths = crate::game::GamePaths::new(base_dir.clone());
                let java_exec = match crate::java::get_java_exec(&base_dir) {
                    Ok(j) => j,
                    Err(e) => {
                        let _ = output.send(Message::GameLaunched(Err(e.to_string()))).await;
                        return;
                    }
                };

                // --- PROXY SETUP (HIJACK MODE) ---
                // Define the paths for the "Hijack"
                let java_real_path = PathBuf::from(&java_exec); // .../bin/java.exe

                // Determine what to run
                let java_exec_for_client;

                if settings.enable_online_fix {
                    println!("[Runner] Enabling Online Fix (Hijack Mode)...");
                    match crate::game::patcher::setup_java_proxy(&java_real_path) {
                        Ok(p) => {
                            java_exec_for_client = p.to_string_lossy().to_string();
                        }
                        Err(e) => {
                            eprintln!("[Runner] Failed to setup Java Proxy: {}", e);
                            // Fallback to original path if proxy setup fails
                            java_exec_for_client = java_real_path.to_string_lossy().to_string();
                        }
                    }
                } else {
                    println!("[Runner] Online Fix Disabled. Ensuring vanilla state...");
                    if let Err(e) = crate::game::patcher::remove_java_proxy(&java_real_path) {
                        eprintln!("[Runner] Failed to remove Java Proxy: {}", e);
                    }
                    java_exec_for_client = java_real_path.to_string_lossy().to_string();
                }

                let version_str = if settings.game_version > 0 {
                    settings.game_version.to_string()
                } else {
                    "latest".to_string()
                };
                let executable_path = paths.client_exe(&settings.channel, &version_str);
                let game_working_dir = paths.version_dir(&settings.channel, &version_str);
                let user_data_dir = paths.user_data();

                // --- PHASE 3: PATCH AND AUTH CONFIGURATION ---
                let mut auth_url = String::new();
                let mut auth_mode = "offline".to_string();

                // Channel to stop the server when the game ends
                let (server_stop_tx, server_stop_rx) = oneshot::channel::<()>();
                let mut server_started = false;

                // Variable to decide what value to send to the DLL
                let mut aurora_env_value = "local".to_string();

                let mut _cleanup_guard: Option<FileCleanupGuard> = None;
                let mut _server_jar_cleanup_guard: Option<ServerJarCleanupGuard> = None;

                let server_port = crate::util::find_free_port();

                if settings.enable_online_fix {
                    auth_mode = "authenticated".to_string();

                    // Write the port to a file for the proxy
                    let port_file = user_data_dir.join("server.port");
                    if let Ok(mut f) = std::fs::File::create(&port_file) {
                        let _ = write!(f, "{}", server_port);
                    }

                    // =========================================================
                    // PARALLEL PATCHING START
                    // =========================================================
                    // Lanzamos el parcheo en segundo plano AHORA, mientras el cliente inicia.
                    // El archivo resultante se guardará en temp_dir para que el proxy lo encuentre.
                    let mut server_jar_path =
                        game_working_dir.join("Server").join("HytaleServer.jar");

                    // Fallback: Check inside Client folder if not found in root
                    if !server_jar_path.exists() {
                        let alt = game_working_dir.join("Client").join("HytaleServer.jar");
                        if alt.exists() {
                            server_jar_path = alt;
                        } else {
                            // Check root
                            let root_alt = game_working_dir.join("HytaleServer.jar");
                            if root_alt.exists() {
                                server_jar_path = root_alt;
                            }
                        }
                    }

                    let original_jar_path = server_jar_path.with_file_name("HytaleServer.original");

                    // 0. Crash Recovery: If .original exists, restore it first to ensure clean state
                    if original_jar_path.exists() {
                        println!("[Runner] Found leftover HytaleServer.original. Restoring...");
                        if server_jar_path.exists() {
                            let _ = std::fs::remove_file(&server_jar_path);
                        }
                        if let Err(e) = std::fs::rename(&original_jar_path, &server_jar_path) {
                            eprintln!("[Runner] Failed to restore original jar: {}", e);
                        }
                    }

                    if !server_jar_path.exists() {
                        println!(
                            "[Runner] ERROR: HytaleServer.jar NOT FOUND at {:?}",
                            server_jar_path
                        );
                        let _ = output.send(Message::GameStopped).await;
                        return;
                    }

                    // Initialize the cleanup guard NOW, so it handles cleanup if we crash/exit
                    _server_jar_cleanup_guard = Some(ServerJarCleanupGuard {
                        original_path: original_jar_path.clone(),
                        current_path: server_jar_path.clone(),
                    });

                    let port_clone = server_port;
                    let server_jar_path_clone = server_jar_path.clone();
                    let original_jar_path_clone = original_jar_path.clone();

                    println!(
                        "[Runner] Scheduling background patch for: {:?}",
                        server_jar_path_clone
                    );

                    // Spawn blocking task (CPU intensive zip ops) in background
                    task::spawn_blocking(move || {
                        if server_jar_path_clone.exists() {
                            println!("[Runner] Starting background server patching (Swap Mode)...");

                            // 1. Patch to a temporary file
                            let patched_tmp_path =
                                server_jar_path_clone.with_file_name("HytaleServer.patched.tmp");

                            match crate::game::patcher::patch_server_jar(
                                &server_jar_path_clone,
                                &patched_tmp_path,
                                port_clone,
                            ) {
                                Ok(_) => {
                                    println!("[Runner] Patch created at: {:?}", patched_tmp_path);

                                    // 2. Rename Original -> .original
                                    if let Err(e) = std::fs::rename(
                                        &server_jar_path_clone,
                                        &original_jar_path_clone,
                                    ) {
                                        eprintln!("[Runner] Failed to backup original jar: {}", e);
                                        // Cleanup temp
                                        let _ = std::fs::remove_file(&patched_tmp_path);
                                        return;
                                    }

                                    // 3. Rename Patched -> .jar
                                    if let Err(e) =
                                        std::fs::rename(&patched_tmp_path, &server_jar_path_clone)
                                    {
                                        eprintln!("[Runner] Failed to install patched jar: {}", e);
                                        // Try to rollback
                                        let _ = std::fs::rename(
                                            &original_jar_path_clone,
                                            &server_jar_path_clone,
                                        );
                                        return;
                                    }

                                    println!(
                                        "[Runner] Successfully swapped HytaleServer.jar with patched version."
                                    );
                                }
                                Err(e) => eprintln!("[Runner] Background patching failed: {}", e),
                            }
                        }
                    });
                    // =========================================================
                    // PARALLEL PATCHING END
                    // =========================================================

                    match settings.online_fix_mode {
                        crate::config::OnlineFixMode::Local => {
                            // Local configuration
                            aurora_env_value = "local".to_string();
                            auth_url = format!("http://127.0.0.000001:{}", server_port);

                            // Start local server
                            let server_username = player_name.clone();
                            let server_uuid = player_uuid.clone();
                            let server_game_dir = game_working_dir.clone();
                            tokio::spawn(async move {
                                crate::game::server::start_server(
                                    server_username,
                                    server_uuid,
                                    server_game_dir,
                                    server_stop_rx,
                                    server_port,
                                )
                                .await;
                            });
                            server_started = true;
                        }
                        crate::config::OnlineFixMode::Sanasol => {
                            // Sanasol configuration
                            auth_url = "https://sessions.sanasol.ws".to_string();
                            aurora_env_value = "sanasol".to_string();

                            // We don't start the local server
                        }
                    }

                    // Extract DLL/SO (common for both modes)
                    #[cfg(target_os = "windows")]
                    {
                        let dll_path = executable_path.parent().unwrap().join("Secur32.dll");
                        if let Ok(mut file) = std::fs::File::create(&dll_path) {
                            let _ = file.write_all(AURORA_BIN);
                        }
                        // Initialize the guard
                        _cleanup_guard = Some(FileCleanupGuard { path: dll_path });
                    }
                    #[cfg(target_os = "linux")]
                    {
                        let natives_dir = base_dir.join("cache").join("natives");
                        if !natives_dir.exists() {
                            let _ = std::fs::create_dir_all(&natives_dir);
                        }
                        let so_path = natives_dir.join("Aurora.so");
                        if let Ok(mut file) = std::fs::File::create(&so_path) {
                            let _ = file.write_all(AURORA_BIN);
                        }
                        // Initialize the guard
                        _cleanup_guard = Some(FileCleanupGuard { path: so_path });
                    }
                } else {
                    // Vanilla/cleanup
                    #[cfg(target_os = "windows")]
                    {
                        let dll_path = executable_path.parent().unwrap().join("Secur32.dll");
                        if dll_path.exists() {
                            let _ = std::fs::remove_file(dll_path);
                        }
                    }
                }

                // --- PHASE 4: TOKEN OBTENTION ---
                let mut auth_args = Vec::new();
                auth_args.push("--auth-mode".to_string());
                auth_args.push(auth_mode.clone());

                if auth_mode == "authenticated" {
                    let tokens = match crate::game::auth::fetch_remote_tokens(
                        &client,
                        &auth_url,
                        &player_name,
                        &player_uuid,
                    )
                    .await
                    {
                        Ok(t) => t,
                        Err(_) => crate::game::auth::generate_fake_tokens(
                            &player_name,
                            &player_uuid,
                            &auth_url,
                        ),
                    };
                    auth_args.extend(vec![
                        "--identity-token".to_string(),
                        tokens.identity_token,
                        "--session-token".to_string(),
                        tokens.session_token,
                    ]);
                }

                // --- PHASE 5: LAUNCH ---
                let _ = output.send(Message::GameLaunched(Ok(()))).await;

                // Prepare environment variables
                let mut envs = std::collections::HashMap::new();

                // Pass the mode to Aurora
                if settings.enable_online_fix {
                    envs.insert("AURORA_MODE".to_string(), aurora_env_value);
                    envs.insert("RUSTALE_IS_PROXY".to_string(), "1".to_string());

                    // --- NEW: Pass the port to Aurora ---
                    // Aurora will read this to know how to replace the string
                    envs.insert("AURORA_PORT".to_string(), server_port.to_string());
                    let logs_dir = base_dir.join("logs");
                    envs.insert(
                        "RUSTALE_LOGS_DIR".to_string(),
                        logs_dir.to_string_lossy().to_string(),
                    );

                    // Pass the logs dir to Aurora
                    let logs_dir = user_data_dir.join("logs");
                    if !logs_dir.exists() {
                        let _ = std::fs::create_dir_all(&logs_dir);
                    }
                    envs.insert(
                        "RUSTALE_LOGS_DIR".to_string(),
                        logs_dir.to_string_lossy().to_string(),
                    );

                    #[cfg(target_os = "linux")]
                    {
                        let natives_dir = base_dir.join("cache").join("natives");
                        let so_path = natives_dir.join("Aurora.so");
                        envs.insert(
                            "LD_PRELOAD".to_string(),
                            so_path.to_string_lossy().to_string(),
                        );
                    }
                }

                if let Ok(mut child) = crate::game::launch_game(
                    &player_name,
                    &player_uuid,
                    &executable_path,
                    &game_working_dir,
                    &user_data_dir,
                    &java_exec_for_client, // <--- USE THE PROXY PATH HERE
                    auth_args,
                    envs,
                ) {
                    // Wait for the game to close
                    let _ = child.wait().await;
                } else {
                    let _ = output
                        .send(Message::GameLaunched(Err(
                            "Failed to spawn game process".into()
                        )))
                        .await;
                }

                // Stop the server when the game closes
                if server_started {
                    let _ = server_stop_tx.send(());
                }

                let _ = output.send(Message::GameStopped).await;
            },
        );
        s.boxed()
    }
}
struct ServerJarCleanupGuard {
    original_path: PathBuf,
    current_path: PathBuf,
}

impl Drop for ServerJarCleanupGuard {
    fn drop(&mut self) {
        if self.original_path.exists() {
            println!("[Cleanup] Restoring original server jar...");
            // Remove the patched one (current HytaleServer.jar)
            if self.current_path.exists() {
                let _ = std::fs::remove_file(&self.current_path);
            }
            // Restore original
            let _ = std::fs::rename(&self.original_path, &self.current_path);
            println!("[Cleanup] Original server jar restored.");
        }
    }
}
