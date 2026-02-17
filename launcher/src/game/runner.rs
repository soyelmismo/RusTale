use crate::Message;
use crate::config::GameSettings;
use crate::game::install::InstallPolicy;
use iced::advanced::subscription::{self, Hasher, Recipe};
use iced::futures::{SinkExt, StreamExt};
use iced::{Subscription, stream};
use sha2::Digest;
use std::hash::Hash;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};
use tokio::sync::mpsc;
use tokio::sync::oneshot;

/// Security guard to clean up temporary files when exiting the scope via Drop.
#[cfg(target_os = "windows")]
struct FileCleanupGuard {
    path: PathBuf,
}

#[cfg(target_os = "windows")]
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

fn verify_aurora_checksum(
    aurora_path: &std::path::Path,
) -> Result<bool, Box<dyn std::error::Error>> {
    // Checksum embebido como variable de entorno en tiempo de compilación
    const EMBEDDED_CHECKSUM: &str = env!("AURORA_CHECKSUM");

    // Calcular checksum del archivo actual
    let aurora_bytes = std::fs::read(aurora_path)?;
    let mut hasher = sha2::Sha256::new();
    hasher.update(&aurora_bytes);
    let current_checksum = format!("{:x}", hasher.finalize());

    let is_valid = EMBEDDED_CHECKSUM == current_checksum;

    if !is_valid {
        eprintln!("[Security] Aurora checksum mismatch!");
        eprintln!("[Security] Expected: {}", EMBEDDED_CHECKSUM);
        eprintln!("[Security] Current:  {}", current_checksum);
    } else {
        println!("[Security] Aurora checksum verified: {}", current_checksum);
    }

    Ok(is_valid)
}

fn ensure_aurora_installed() -> Result<(), String> {
    let tools_dir = crate::config::get_app_dir().join("tools");

    // Asegurar que el directorio tools exista
    if !tools_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&tools_dir) {
            return Err(format!("Failed to create tools directory: {}", e));
        }
    }

    let aurora_lib = format!("aurora{}", std::env::consts::DLL_SUFFIX);

    let aurora_path = tools_dir.join(&aurora_lib);

    // Paso 1: Verificar si Aurora ya existe en tools/ con el checksum correcto
    if aurora_path.exists() {
        match verify_aurora_checksum(&aurora_path) {
            Ok(true) => {
                println!("[Aurora] Found valid Aurora binary in tools/");
                return Ok(());
            }
            Ok(false) => {
                println!("[Aurora] Existing Aurora binary in tools/ has invalid checksum");
            }
            Err(e) => {
                return Err(format!("Failed to verify Aurora checksum in tools/: {}", e));
            }
        }
    }

    // Paso 2: Buscar Aurora junto al ejecutable y copiarlo si es válido
    let exe_path = match std::env::current_exe() {
        Ok(path) => path,
        Err(e) => return Err(format!("Cannot get executable path: {}", e)),
    };

    let exe_dir = match exe_path.parent() {
        Some(dir) => dir,
        None => return Err("Cannot get executable directory".to_string()),
    };

    let source_path = exe_dir.join(&aurora_lib);

    if source_path.exists() {
        println!("[Aurora] Found Aurora binary alongside executable, verifying...");
        match verify_aurora_checksum(&source_path) {
            Ok(true) => match std::fs::copy(&source_path, &aurora_path) {
                Ok(_) => {
                    println!("[Aurora] Copied valid Aurora binary to tools/");
                    return Ok(());
                }
                Err(e) => return Err(format!("Failed to copy Aurora to tools/: {}", e)),
            },
            Ok(false) => {
                println!("[Aurora] Aurora binary alongside executable has invalid checksum");
            }
            Err(e) => {
                return Err(format!(
                    "Failed to verify Aurora checksum alongside executable: {}",
                    e
                ));
            }
        }
    }

    // Paso 3: No se encontró Aurora válido en ninguna ubicación
    Err(format!(
        "Aurora binary not found or invalid. Expected locations:\n  - {:?}\n  - {:?}",
        aurora_path, source_path
    ))
}

pub fn run(
    settings: GameSettings,
    player_name: String,
    player_uuid: String,
    client: reqwest::Client,
    target_version: Option<i32>,
    install_policy: InstallPolicy,
    cancel_token: Arc<AtomicBool>,
) -> Subscription<Message> {
    subscription::from_recipe(Runner {
        settings,
        player_name,
        player_uuid,
        client,
        target_version,
        install_policy,
        cancel_token,
    })
}

struct Runner {
    settings: GameSettings,
    player_name: String,
    player_uuid: String,
    client: reqwest::Client,
    target_version: Option<i32>,
    install_policy: InstallPolicy,
    cancel_token: Arc<AtomicBool>,
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
        // cancel_token is not hashed as it's a dynamic signal
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
        let cancel_token = self.cancel_token;

        let s = stream::channel::<Message>(
            100,
            move |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
                let base_dir = crate::config::get_app_dir();

                // --- PHASE 1: VERIFICATION / INSTALLATION ---
                let (tx, mut rx) = mpsc::channel(100);
                let (res_tx, mut res_rx) = mpsc::channel(1);

                let install_settings = settings.clone();
                let install_client = client.clone();
                let install_base = base_dir.clone();
                let install_base_clone = install_base.clone();
                let install_channel_clone = install_settings.channel.clone();

                tokio::spawn(async move {
                    let progress_tx = tx.clone();
                    let progress_tx_for_steps = tx.clone(); // Clone extra para UpdateTotalSteps
                    let progress_tx_for_error = tx.clone(); // Clone extra para DownloadError

                    // Create the unified progress reporter
                    let progress_reporter =
                        move |payload: crate::game::progress::ProgressPayload| {
                            let _ = progress_tx.try_send(Message::ProgressUpdate(payload));
                        };

                    let result = crate::game::patch_api::PatchApiFrontend::get_instance()
                        .ensure_installed_with_weighted_progress(
                            &install_client,
                            &install_base,
                            &install_settings.channel,
                            if install_settings.game_version > 0 {
                                Some(install_settings.game_version as i32)
                            } else {
                                Some(0)
                            },
                            install_policy,
                            progress_reporter,
                            Some(cancel_token),
                        )
                        .await;

                    match result {
                        Ok((total_steps, ())) => {
                            // Update UI with step information
                            let _ = progress_tx_for_steps.try_send(Message::UpdateTotalSteps {
                                total_steps: Some(total_steps),
                            });
                        }
                        Err(ref e) => {
                            println!("[Install] Installation failed: {}", e);

                            // Detectar específicamente errores de cancelación
                            let is_cancelled = e.to_string().contains("cancelled")
                                || e.to_string().contains("Cancelled")
                                || e.to_string().contains("cancel");

                            if is_cancelled {
                                println!("[Install] Installation cancelled by user");
                                // Para cancelación, solo enviar GameStopped, no DownloadError
                                let _ = progress_tx_for_error.try_send(Message::GameStopped);
                            } else {
                                // Para otros errores, enviar DownloadError como antes
                                let _ = progress_tx_for_error
                                    .try_send(Message::DownloadError(e.to_string()));
                            }
                        }
                    }
                    let _ = res_tx.send(result).await;
                });

                while let Some(msg) = rx.recv().await {
                    let _ = output.send(msg).await;
                }

                match res_rx.recv().await {
                    Some(Ok(_)) => {
                        println!(
                            "[Install] Installation completed successfully, preparing to launch"
                        );

                        // Save version.json for latest installations
                        if install_settings.game_version == 0 {
                            let version_info =
                                crate::game::patch_api::PatchApiFrontend::get_instance()
                                    .get_version_info(
                                        &install_base_clone,
                                        &install_channel_clone,
                                        0,
                                    )
                                    .await;
                            if let Ok(info) = version_info {
                                if let Err(e) = crate::game::install::save_local_version(
                                    &install_base_clone,
                                    &install_channel_clone,
                                    info.latest_remote,
                                )
                                .await
                                {
                                    println!("[WARNING] Failed to save version.json: {}", e);
                                } else {
                                    println!(
                                        "[INFO] Saved version.json for latest version: {}",
                                        info.latest_remote
                                    );
                                }
                            }
                        }

                        let _ = output
                            .send(Message::ProgressUpdate(
                                crate::game::progress::ProgressPayload {
                                    global_progress: 1.0, // 100%
                                    step_progress: 1.0,   // 100%
                                    message_key: "launcher.status.preparing_launch".to_string(),
                                    message_args: vec![],
                                    stats: None,
                                },
                            ))
                            .await;
                    }
                    Some(Err(ref e)) => {
                        println!("[Install] Installation failed with error: {}", e);
                        let _ = output.send(Message::GameStopped).await;
                        return;
                    }
                    None => {
                        println!("[Install] Installation result channel closed unexpectedly");
                        let _ = output.send(Message::GameStopped).await;
                        return;
                    }
                }

                // --- PHASE 2: PATH PREPARATION ---
                let paths = crate::game::GamePaths::new(base_dir.clone());
                let java_exec = match crate::java::get_java_exec(&base_dir) {
                    Ok(j) => j,
                    Err(e) => {
                        let _ = output
                            .send(Message::GameLaunched(Err(format!("Java Error: {}", e))))
                            .await;
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
                    match crate::java::proxy::setup_java_proxy(&java_real_path) {
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
                    if let Err(e) = crate::java::proxy::remove_java_proxy(&java_real_path) {
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

                let mut server_port = crate::util::get_saved_port();
                if !auth_server::is_server_alive(server_port).await {
                    server_port = crate::util::find_free_port();
                }

                if settings.enable_online_fix {
                    auth_mode = "authenticated".to_string();

                    // 1. Guardar el puerto actual en RAM inmediatamente
                    crate::util::save_active_port(server_port);

                    // --- [CORRECCIoN INICIO] ---
                    // Configuramos los modos y URLs ANTES de verificar archivos fisicos.
                    match settings.online_fix_mode {
                        crate::config::OnlineFixMode::Local => {
                            aurora_env_value = "local".to_string();
                            auth_url = format!("http://127.0.0.000001:{}", server_port);
                        }
                        crate::config::OnlineFixMode::Sanasol => {
                            aurora_env_value = "sanasol".to_string();
                            auth_url = "https://sessions.sanasol.ws".to_string();
                        }
                    }
                    // --- [CORRECCIoN FIN] ---

                    let server_root_dir = crate::config::get_server_root_dir();
                    if !server_root_dir.exists() {
                        let _ = std::fs::create_dir_all(&server_root_dir);
                    }

                    let port_file = server_root_dir.join("server.port");
                    if let Ok(mut f) = std::fs::File::create(&port_file) {
                        let _ = write!(f, "{}", server_port);
                    }

                    // =========================================================
                    // PARALLEL PATCHING START (PERSISTENT & NON-DESTRUCTIVE)
                    // =========================================================
                    let mut server_jar_path =
                        game_working_dir.join("Server").join("HytaleServer.jar");
                    let mut server_dir = game_working_dir.join("Server");

                    // Fallback: Check locations
                    if !server_jar_path.exists() {
                        let locations = [
                            game_working_dir.join("Server").join("HytaleServer.jar"),
                            game_working_dir.join("HytaleServer.jar"),
                        ];
                        for loc in locations {
                            if loc.exists() {
                                server_jar_path = loc;
                                server_dir = server_jar_path
                                    .parent()
                                    .map(|p| p.to_path_buf())
                                    .unwrap_or_else(|| game_working_dir.clone());
                                break;
                            }
                        }
                    }

                    // Verificamos si al menos el server_dir existe
                    if server_dir.exists() {
                        println!(
                            "[Runner] Server directory found. No JAR patching needed (DualAuth Agent active)."
                        );
                    } else {
                        eprintln!(
                            "[Runner] WARNING: Server directory NOT FOUND at {:?}",
                            server_dir
                        );
                        // We continue anyway as the agent is runtime-based, but logs might complain
                    }

                    // --- [RESTORED]: Authentication & Server Logic ---
                    if settings.online_fix_mode == crate::config::OnlineFixMode::Local {
                        // NOTA: Estos clones son necesarios porque las variables se usan
                        // tanto en el async block como más adelante en la función
                        let server_username = player_name.clone();
                        let server_uuid = player_uuid.clone();
                        let server_game_dir = game_working_dir.clone();
                        let port_clone = server_port;

                        if !auth_server::is_server_alive(port_clone).await {
                            println!("[Runner] Starting Auth Server on port {}", port_clone);
                            tokio::spawn(async move {
                                let identity_dir = crate::config::get_identity_dir();
                                let _ = auth_server::start_server(
                                    server_username,
                                    server_uuid,
                                    server_game_dir,
                                    identity_dir,
                                    server_stop_rx,
                                    port_clone,
                                )
                                .await;
                            });
                            // Give it a moment to bind
                            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        } else {
                            println!(
                                "[Runner] Connected to existing Auth Server. Updating state..."
                            );

                            let client_sync = reqwest::Client::new();
                            let base_url = format!("http://127.0.0.000001:{}", port_clone);

                            // 1. Actualizar Path
                            let update_path_url = format!("{}/internal/update-path", base_url);
                            let body_path = serde_json::json!({
                                "game_dir": server_game_dir.to_string_lossy().to_string()
                            });
                            let _ = client_sync
                                .post(update_path_url)
                                .json(&body_path)
                                .send()
                                .await;

                            // 2. Actualizar Identidad
                            let update_id_url = format!("{}/internal/update-identity", base_url);
                            let body_id = serde_json::json!({
                                "username": server_username,
                                "uuid": server_uuid
                            });
                            let _ = client_sync.post(update_id_url).json(&body_id).send().await;
                        }
                        server_started = true;
                    }
                } else {
                    // Vanilla/cleanup - redundant now but kept for safety
                    #[cfg(target_os = "windows")]
                    {
                        let dll_path = executable_path
                            .parent()
                            .map(|p| p.join("Secur32.dll"))
                            .unwrap_or_else(|| executable_path.join("Secur32.dll"));
                        if dll_path.exists() {
                            let _ = std::fs::remove_file(dll_path);
                        }
                    }
                }

                // =========================================================
                // PHASE 3 END
                // =========================================================

                // --- PHASE 3.5: AURORA INSTALLATION ---
                if settings.enable_online_fix {
                    if let Err(e) = ensure_aurora_installed() {
                        eprintln!("[Runner] Failed to ensure Aurora is installed: {}", e);
                        let _ = output
                            .send(Message::GameLaunched(Err(format!("Aurora Error: {}", e))))
                            .await;
                        return;
                    }
                }

                // Aurora ya está disponible en tools/aurora{DLL_SUFFIX}
                // En Windows se copia a la carpeta del cliente como Secur32.dll
                if settings.enable_online_fix {
                    let tools_aurora_path = crate::config::get_app_dir()
                        .join("tools")
                        .join(format!("aurora{}", std::env::consts::DLL_SUFFIX));

                    // Verificar que Aurora exista
                    if !tools_aurora_path.exists() {
                        eprintln!(
                            "[Runner] Aurora binary not found at {:?}",
                            tools_aurora_path
                        );
                        return;
                    }

                    #[cfg(target_os = "windows")]
                    {
                        let dll_path = executable_path
                            .parent()
                            .map(|p| p.join("Secur32.dll"))
                            .unwrap_or_else(|| executable_path.join("Secur32.dll"));

                        if let Err(e) = std::fs::copy(&tools_aurora_path, &dll_path) {
                            eprintln!("[Runner] Failed to copy Aurora binary: {}", e);
                            return;
                        }

                        // Initialize the guard
                        let security_cleanup_guard = FileCleanupGuard { path: dll_path };
                        println!("[Runner] Security cleanup guard initialized for {:?}", security_cleanup_guard.path);
                        // Store it in a variable that lasts the scope
                        let _active_guard = security_cleanup_guard;
                        println!("[Runner] Aurora copied to Secur32.dll");
                    }

                    #[cfg(target_os = "linux")]
                    {
                        println!("[Runner] Using Aurora binary from tools/");
                    }
                }

                // --- PHASE 4: TOKEN OBTENTION ---
                let mut auth_args = Vec::new();
                auth_args.push("--auth-mode".to_string());
                auth_args.push(auth_mode.clone());

                if auth_mode == "authenticated" {
                    // --- Sincronizar JWKS Remotos (BLOQUEANTE AQUi PARA EVITAR RACE CONDITIONS) ---
                    match crate::game::auth::fetch_remote_jwks(&client, &auth_url).await {
                        Ok(jwks) => {
                            auth_server::crypto::update_jwks_from_remote(jwks);
                        }
                        Err(e) => {
                            eprintln!(
                                "[Runner] Warning: Could not sync remote JWKS: {}. If the server just started, this is critical.",
                                e
                            );
                        }
                    }

                    // Retry loop for fetching tokens from the dedicated server/emulator
                    let mut tokens_res = Err(anyhow::anyhow!("Initial state"));
                    for i in 0..5 {
                        match crate::game::auth::fetch_remote_tokens(
                            &client,
                            &auth_url,
                            &player_name,
                            &player_uuid,
                        )
                        .await
                        {
                            Ok(t) => {
                                tokens_res = Ok(t);
                                break;
                            }
                            Err(e) => {
                                println!(
                                    "[Runner] Auth fetch attempt {} failed: {}. Retrying...",
                                    i + 1,
                                    e
                                );
                                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                            }
                        }
                    }

                    let tokens = match tokens_res {
                        Ok(t) => t,
                        Err(e) => {
                            println!(
                                "[Runner] CRITICAL: Failed to fetch remote tokens after retries: {}",
                                e
                            );
                            // If it fails, generating fake tokens is a "last resort" but
                            // it will likely fail on the server if keys mismatch.
                            crate::game::auth::generate_fake_tokens(
                                &player_name,
                                &player_uuid,
                                &auth_url,
                            )
                        }
                    };
                    auth_args.extend(vec![
                        "--identity-token".to_string(),
                        tokens.identity_token,
                        "--session-token".to_string(),
                        tokens.session_token,
                    ]);
                }

                // --- PHASE 4.5: MODS SYNC ---
                // Synchronize the current version's jar mods to UserData/Mods
                {
                    let version_mods_src = paths.mods_dir(&settings.channel, &version_str);
                    let global_mods_target = user_data_dir.join("Mods");

                    println!("[Runner] Syncing mods to UserData/Mods...");

                    // 1. Clean destination folder (avoid mixing mods from other versions)
                    if global_mods_target.exists() {
                        if let Err(e) = tokio::fs::remove_dir_all(&global_mods_target).await {
                            eprintln!("[Runner] Warning: Failed to clean UserData/Mods: {}", e);
                        }
                    }
                    if let Err(e) = tokio::fs::create_dir_all(&global_mods_target).await {
                        eprintln!("[Runner] Error creating UserData/Mods: {}", e);
                    }

                    // 2. Copy enabled mods
                    if version_mods_src.exists() {
                        if let Ok(mut entries) = tokio::fs::read_dir(&version_mods_src).await {
                            while let Ok(Some(entry)) = entries.next_entry().await {
                                let path = entry.path();
                                if path.is_file() {
                                    if let Some(name) = path.file_name() {
                                        let dest_path = global_mods_target.join(name);
                                        // Simple copy
                                        if let Err(e) = tokio::fs::copy(&path, &dest_path).await {
                                            eprintln!(
                                                "[Runner] Failed to copy mod {:?}: {}",
                                                name, e
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // --- PHASE 5: LAUNCH ---
                let _ = output.send(Message::GameLaunched(Ok(()))).await;

                // Prepare environment variables
                let mut envs = std::collections::HashMap::new();

                // Pass the mode to Aurora
                if settings.enable_online_fix {
                    // 2. Variables de entorno minimas para el cliente (REDUNDANCIA DE SEGURIDAD)
                    envs.insert("AURORA_MODE".to_string(), aurora_env_value);
                    envs.insert("RUSTALE_IS_PROXY".to_string(), "1".to_string());
                    envs.insert("AURORA_PORT".to_string(), server_port.to_string());

                    // Disable Sentry
                    envs.insert("DISABLE_SENTRY".to_string(), "1".to_string());

                    // --- NUEVO: DUAL AUTH CONFIG ---
                    if settings.online_fix_mode == crate::config::OnlineFixMode::Local {
                        envs.insert(
                            "HYTALE_AUTH_DOMAIN".to_string(),
                            format!("127.0.0.000001:{}", server_port),
                        );
                    } else {
                        envs.insert(
                            "HYTALE_AUTH_DOMAIN".to_string(),
                            "sessions.sanasol.ws".to_string(),
                        );
                    }
                    // ------------------------------

                    let logs_dir = base_dir.join("logs");
                    if !logs_dir.exists() {
                        let _ = std::fs::create_dir_all(&logs_dir);
                    }
                    envs.insert(
                        "RUSTALE_LOGS_DIR".to_string(),
                        logs_dir.to_string_lossy().to_string(),
                    );

                    #[cfg(target_os = "linux")]
                    {
                        let so_path = crate::config::get_app_dir().join("tools").join("aurora.so");

                        // En lugar de sobreescribir LD_PRELOAD, es mejor añadirla.
                        // Algunos sistemas tienen sus propias precargas (fakeroot, steam overlay, etc).
                        if let Ok(current_preload) = std::env::var("LD_PRELOAD") {
                            let new_preload =
                                format!("{}:{}", so_path.to_string_lossy(), current_preload);
                            envs.insert("LD_PRELOAD".to_string(), new_preload);
                        } else {
                            envs.insert(
                                "LD_PRELOAD".to_string(),
                                so_path.to_string_lossy().to_string(),
                            );
                        }
                    }
                }

                // --- LINUX SPECIFIC FIXES ---
                #[cfg(target_os = "linux")]
                {
                    if let Some(bin_dir) = executable_path.parent() {
                        let bundled_lib = bin_dir.join("libzstd.so");
                        let backup_lib = bin_dir.join("libzstd.so.bundled");

                        // Lista de rutas comunes donde Ubuntu/Fedora/Arch guardan la libreria buena
                        let system_paths = [
                            "/usr/lib/x86_64-linux-gnu/libzstd.so.1", // Debian/Ubuntu moderno
                            "/usr/lib64/libzstd.so.1",                // Fedora/RHEL
                            "/usr/lib/libzstd.so.1",                  // Arch/SteamDeck
                            "/lib/x86_64-linux-gnu/libzstd.so.1",     // Fallback Ubuntu
                        ];

                        // Buscar cual existe en tu sistema
                        let system_lib = system_paths
                            .iter()
                            .find(|p| std::path::Path::new(p).exists());

                        if let Some(sys_path) = system_lib {
                            println!("[Linux-Fix] Found system zstd at: {}", sys_path);

                            // 1. Respaldar la libreria corrupta que trae el juego (si es un archivo real)
                            if bundled_lib.exists() {
                                let is_symlink = std::fs::symlink_metadata(&bundled_lib)
                                    .map(|m| m.file_type().is_symlink())
                                    .unwrap_or(false);

                                // Si es archivo real, lo renombramos a .bundled
                                if !is_symlink {
                                    println!("[Linux-Fix] Backing up bundled libzstd.so...");
                                    let _ = std::fs::rename(&bundled_lib, &backup_lib);
                                }
                            }

                            // 2. Crear el Symlink magico si no existe o estaba roto
                            // Si no existe (porque lo acabamos de renombrar), lo creamos.
                            if !bundled_lib.exists() {
                                println!(
                                    "[Linux-Fix] Creating symlink: Local/libzstd.so -> System/libzstd.so.1"
                                );
                                use std::os::unix::fs::symlink;
                                if let Err(e) = symlink(sys_path, &bundled_lib) {
                                    eprintln!("[Linux-Fix] Failed to create symlink: {}", e);
                                }
                            }
                        } else {
                            eprintln!(
                                "[Linux-Fix] WARNING: System libzstd.so.1 NOT FOUND. Install 'libzstd1' via apt/pacman."
                            );
                        }
                    }
                }

                if let Ok(mut child) = crate::game::launch_game_with_async_agent(
                    &player_name,
                    &player_uuid,
                    &executable_path,
                    &game_working_dir,
                    &user_data_dir,
                    &java_exec_for_client, // <--- USE THE PROXY PATH HERE
                    auth_args,
                    envs,
                    client.clone(),
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
