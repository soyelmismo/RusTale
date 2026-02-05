use crate::config::OnlineFixMode;
use crate::server::assets::{
    find_best_client_version, generate_server_args_with_direct_assets, validate_client_version,
};
use crate::server::config::ServerConfig;
use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::process::Command;
use tokio::sync::mpsc;

pub async fn run_server_flow(mut config: ServerConfig) -> Result<()> {
    println!("--- RusTale Dedicated Server ---");
    println!(
        "Mode: {} | Port: 5520 | Version: {}",
        config.online_mode, config.game_version
    );

    // AUTO-SINCRO: Buscamos el puerto dinamico de la instancia madre
    let mut auth_port = crate::util::get_saved_port();

    if crate::game::server::is_server_alive(auth_port).await {
        println!("[RusTale] Local Auth found on port {}", auth_port);
        println!("[RusTale] Attaching to existing emulator instance...");
    } else {
        // Si el puerto guardado esta muerto, buscamos uno nuevo libre
        auth_port = crate::util::find_free_port();
        println!(
            "[RusTale] No active instance found. Starting new emulator on port {}",
            auth_port
        );

        // GUARDAMOS EL SECRETO: Escribimos el puerto para que Aurora (LD_PRELOAD) lo encuentre
        crate::util::save_active_port(auth_port);
    }

    // 1. Definir Rutas (Unificadas a root_dir para evitar OS Error 3)
    let root_dir = crate::config::get_server_root_dir();
    let _ = tokio::fs::create_dir_all(&root_dir).await;

    // -----------------------------------------------------------------------
    // INICIAR SERVIDOR WEB LOCAL (Si es necesario)
    // -----------------------------------------------------------------------
    let (auth_result_tx, mut auth_result_rx) = tokio::sync::oneshot::channel();
    let (_server_stop_tx, server_stop_rx) = tokio::sync::oneshot::channel::<()>();

    if config.online_mode == "local" {
        if crate::game::server::is_server_alive(auth_port).await {
            println!(
                "Local Auth Server already running on port {}. Attaching...",
                auth_port
            );

            // Sincronizar llaves para que el proceso actual reconozca los tokens del emulador activo
            let client = reqwest::Client::new();
            let auth_url = format!("http://127.0.0.000001:{}", auth_port);
            match crate::game::auth::fetch_remote_jwks(&client, &auth_url).await {
                Ok(jwks) => {
                    crate::game::crypto::update_jwks_from_remote(jwks);
                }
                Err(e) => {
                    eprintln!(
                        "[Server] Warning: Could not sync JWKS from existing emulator: {}",
                        e
                    );
                }
            }

            let _ = auth_result_tx.send(Ok(()));
        } else {
            println!(
                "Starting Local Auth Server (Emulator) on port {}...",
                auth_port
            );

            // ... (setup host_uuid and host_name) ...
            let host_uuid = uuid::Uuid::new_v4().to_string();
            let host_name = "ConsoleHost".to_string();

            let version_dir_name = if config.game_version == "latest" || config.game_version == "0"
            {
                "latest".to_string()
            } else {
                config.game_version.clone()
            };
            let install_dir_ref = root_dir.join(&config.branch).join(&version_dir_name);

            tokio::spawn(async move {
                // Try to start the server
                let res = crate::game::server::start_server(
                    host_name,
                    host_uuid,
                    install_dir_ref,
                    server_stop_rx,
                    auth_port,
                )
                .await;

                // Notify the main thread the result
                let _ = auth_result_tx.send(res);
            });

            // Give it a small break to start or fail
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    } else {
        let _ = auth_result_tx.send(Ok(()));
    }

    // Check if the Auth Server failed before downloading anything
    // If the port is occupied by a zombie, this will fail here and stop everything
    if let Ok(Err(e)) = auth_result_rx.try_recv() {
        eprintln!("\n[FATAL] Could not start authentication server: {}", e);
        eprintln!("It is likely that a previous instance was left open.");
        eprintln!("Please close 'java' or 'rustale' from Task Manager.\n");
        std::process::exit(1);
    }

    let _tools_dir = root_dir.join("tools");
    let version_dir_name = if config.game_version == "latest" || config.game_version == "0" {
        "latest".to_string()
    } else {
        config.game_version.clone()
    };

    let install_dir = root_dir.join(&config.branch).join(&version_dir_name);

    let client = reqwest::Client::builder()
        .user_agent(format!("RusTale-Server/{}", env!("CARGO_PKG_VERSION")))
        .build()?;

    // 0. Ensure DualAuth Agent
    println!("Ensuring DualAuth Agent is up to date...");
    crate::game::agent::ensure_agent(
        &reqwest::Client::new(),
        &root_dir,
        &|_, _, _| {}, // Silent progress for CLI
        None,
    )
    .await?;

    // 1. Tool Validation (JRE e Itch/Butler)
    println!("[1/5] Validating tools...");
    let callback = |task: &str, pct: f64, msg: &str| {
        if pct == 0.0 || pct == 100.0 {
            println!("[{}] {}", task, msg);
        }
    };

    // --- OPTIMIZATION: REUSE MAIN INSTALLATION TOOLS ---
    let main_app_dir = crate::config::get_app_dir();
    let tools_dir = main_app_dir.join("tools");
    let main_java = tools_dir.join("jre");
    let main_butler = tools_dir.join("butler");
    let server_java = _tools_dir.join("jre");
    let server_butler = _tools_dir.join("butler");

    if main_java.exists()
        && (!server_java.exists() || crate::java::get_java_exec(&root_dir).is_err())
    {
        println!("[Tools] Copying JRE from main installation...");
        let mr = main_java.clone();
        let sr = server_java.clone();
        tokio::task::spawn_blocking(move || crate::util::copy_recursive_sync(mr, sr)).await??;
    }

    if main_butler.exists() && !server_butler.exists() {
        println!("[Tools] Copying Butler from main installation...");
        let mt = main_butler.clone();
        let st = server_butler.clone();
        tokio::task::spawn_blocking(move || crate::util::copy_recursive_sync(mt, st)).await??;
    }

    crate::java::download_jre(&client, &root_dir, &callback, None).await?;
    let _butler_path =
        crate::game::patcher::install_butler(&client, &root_dir, &callback, None).await?;

    // 3. Resolver Version y Descargar Servidor
    println!("[2/5] Checking Game Server files...");

    let target_ver_num = if config.game_version == "latest" {
        crate::game::patcher::find_latest_version(&client, &config.branch).await?
    } else {
        config
            .game_version
            .parse::<i32>()
            .context("Invalid version number")?
    };

    let server_jar_raw = install_dir.join("Server").join("HytaleServer.jar");

    // --- OPTIMIZATION: REUSE MAIN INSTALLATION GAME FILES ---
    if !server_jar_raw.exists() {
        println!("Checking for local game files in main installation...");
        let mut source_candidate = None;

        // Find the best matching client version
        let best_version = match find_best_client_version(
            &main_app_dir,
            &config.branch,
            &config.game_version.to_string(),
        ) {
            Ok(version) => {
                println!("Found matching client version: {}", version);
                version
            }
            Err(e) => {
                println!("Warning: {}", e);
                // Fallback to original logic
                let target_ver_num = if config.game_version == "latest" {
                    crate::game::patcher::find_latest_version(&client, &config.branch).await?
                } else {
                    config
                        .game_version
                        .parse::<i32>()
                        .context("Invalid version number")?
                };
                target_ver_num.to_string()
            }
        };

        // Candidate 1: Specific version folder
        let specific = main_app_dir.join(&config.branch).join(&best_version);
        if specific.exists() && specific.join("Server").exists() {
            source_candidate = Some(specific);
        }

        // Candidate 2: Latest folder (check version.json)
        if source_candidate.is_none() && best_version == "latest" {
            let latest = main_app_dir.join(&config.branch).join("latest");
            if latest.exists() {
                if let Ok(ver) =
                    crate::game::install::get_local_version(&main_app_dir, &config.branch).await
                {
                    if ver.to_string() == config.game_version {
                        source_candidate = Some(latest);
                    }
                }
            }
        }

        if let Some(src) = source_candidate {
            println!(
                "Found matching version {} at {:?}. Processing files...",
                best_version, src
            );
            let _ = tokio::fs::create_dir_all(&install_dir).await;

            // Handle Assets.zip based on use_direct_assets setting
            let src_assets = src.join("Assets.zip");
            if config.use_direct_assets {
                // Validate client version has required files
                if let Err(e) =
                    validate_client_version(&main_app_dir, &config.branch, &best_version)
                {
                    eprintln!("Client validation failed: {}", e);
                    eprintln!("Falling back to copying Assets.zip...");
                    // Fallback to copying if validation fails
                    let dst_assets = install_dir.join("Assets.zip");
                    if src_assets.exists() && !dst_assets.exists() {
                        println!("Copying Assets.zip (~3GB) as fallback...");
                        if let Err(e) = tokio::fs::copy(&src_assets, &dst_assets).await {
                            eprintln!("Failed to copy Assets.zip: {}", e);
                        }
                    }
                    // Add --assets argument for local copy
                    config.server_args =
                        generate_server_args_with_direct_assets(&config.server_args, &dst_assets);
                } else {
                    println!("Using assets directly from client installation (no copying)");
                    println!("Assets path: {:?}", src_assets);
                    // Add --assets argument with absolute path to client assets
                    config.server_args =
                        generate_server_args_with_direct_assets(&config.server_args, &src_assets);
                }
            } else {
                // Original behavior: copy Assets.zip
                let dst_assets = install_dir.join("Assets.zip");
                if src_assets.exists() && !dst_assets.exists() {
                    println!("Copying Assets.zip (~3GB)...");
                    if let Err(e) = tokio::fs::copy(&src_assets, &dst_assets).await {
                        eprintln!("Failed to copy Assets.zip: {}", e);
                    }
                } else if !src_assets.exists() {
                    eprintln!("WARNING: Source Assets.zip not found at {:?}", src_assets);
                    eprintln!("This may cause Exit Code 7 - server will fail without assets!");
                }
                // Add --assets argument for local copy
                config.server_args =
                    generate_server_args_with_direct_assets(&config.server_args, &dst_assets);
            }

            // Copy Server folder (always needed)
            let src_server = src.join("Server");
            let dst_server = install_dir.join("Server");
            if src_server.exists() && !dst_server.exists() {
                println!("Copying Server folder...");
                // Use blocking thread for recursive copy
                let dst_s_clone = dst_server.clone();
                let res = tokio::task::spawn_blocking(move || {
                    crate::util::copy_recursive_sync(src_server, dst_s_clone)
                })
                .await?;

                if let Err(e) = res {
                    eprintln!("Failed to copy Server folder: {}", e);
                } else {
                    println!("Files copied successfully!");

                    // CLEANUP: If we copied a patched server jar, restore the original
                    let potential_original = dst_server.join("HytaleServer.original");
                    let potential_jar = dst_server.join("HytaleServer.jar");

                    if potential_original.exists() {
                        println!("Found local .original backup. Restoring vanilla state...");
                        if let Err(e) = tokio::fs::rename(&potential_original, &potential_jar).await
                        {
                            eprintln!("Failed to restore original JAR: {}", e);
                        }
                    }
                }
            }
        }
    }
    // --------------------------------------------------------

    // Handle assets configuration - prioritize local server assets first
    println!("Setting up assets configuration...");
    let local_assets_path = install_dir.join("Assets.zip");
    let mut source_candidate = None;

    // Find the best matching client version for assets (moved to higher scope)
    let main_app_dir = crate::config::get_app_dir();
    let best_version = match find_best_client_version(
        &main_app_dir,
        &config.branch,
        &config.game_version.to_string(),
    ) {
        Ok(version) => {
            println!("Found matching client version for assets: {}", version);
            version
        }
        Err(e) => {
            println!("Warning: {}", e);
            config.game_version.clone()
        }
    };

    if local_assets_path.exists() {
        println!("[Assets] Using existing Assets.zip found in server directory.");
        // Force specific server args for the existing local file
        config.server_args =
            generate_server_args_with_direct_assets(&config.server_args, &local_assets_path);
    } else {
        println!("Assets not found in server dir, searching client installation...");

        // Try to find client installation
        // Candidate 1: Specific version folder
        let specific = main_app_dir.join(&config.branch).join(&best_version);
        if specific.exists() && specific.join("Assets.zip").exists() {
            source_candidate = Some(specific);
        }

        // Candidate 2: Latest folder
        if source_candidate.is_none() && best_version == "latest" {
            let latest = main_app_dir.join(&config.branch).join("latest");
            if latest.exists() && latest.join("Assets.zip").exists() {
                source_candidate = Some(latest);
            }
        }

        // Handle assets based on source and configuration
        if let Some(src) = source_candidate {
            let src_assets = src.join("Assets.zip");
            println!("Found client assets at: {:?}", src_assets);

            if config.use_direct_assets {
                // Validate client version has required files
                if let Err(e) =
                    validate_client_version(&main_app_dir, &config.branch, &best_version)
                {
                    eprintln!("Client validation failed: {}", e);
                    eprintln!("Falling back to copying Assets.zip...");
                    // Fallback to copying if validation fails
                    if src_assets.exists() && !local_assets_path.exists() {
                        println!("Copying Assets.zip (~3GB) as fallback...");
                        if let Err(e) = tokio::fs::copy(&src_assets, &local_assets_path).await {
                            eprintln!("Failed to copy Assets.zip: {}", e);
                        }
                    }
                    // Add --assets argument for local copy
                    config.server_args = generate_server_args_with_direct_assets(
                        &config.server_args,
                        &local_assets_path,
                    );
                } else {
                    println!("Using assets directly from client installation (no copying)");
                    println!("Assets path: {:?}", src_assets);
                    // Add --assets argument with absolute path to client assets
                    config.server_args =
                        generate_server_args_with_direct_assets(&config.server_args, &src_assets);
                }
            } else {
                // Original behavior: copy Assets.zip
                if src_assets.exists() && !local_assets_path.exists() {
                    println!("Copying Assets.zip (~3GB)...");
                    if let Err(e) = tokio::fs::copy(&src_assets, &local_assets_path).await {
                        eprintln!("Failed to copy Assets.zip: {}", e);
                    }
                } else if !src_assets.exists() {
                    eprintln!("WARNING: Source Assets.zip not found at {:?}", src_assets);
                    eprintln!("This may cause Exit Code 7 - server will fail without assets!");
                }
                // Add --assets argument for local copy
                config.server_args = generate_server_args_with_direct_assets(
                    &config.server_args,
                    &local_assets_path,
                );
            }
        } else {
            eprintln!("WARNING: No client assets found! Server may fail to start.");
            eprintln!(
                "Looking for assets in: {:?}",
                main_app_dir
                    .join(&config.branch)
                    .join(&best_version)
                    .join("Assets.zip")
            );
        }
    }

    // JAR recovery fallback - check for .original backup before downloading
    let server_original_fallback = install_dir.join("Server").join("HytaleServer.original");

    // New pre-check logic
    let jar_available = if server_jar_raw.exists() {
        true
    } else if server_original_fallback.exists() {
        println!("[Recovery] JAR missing but .original found. Restoring vanilla state...");
        tokio::fs::copy(&server_original_fallback, &server_jar_raw)
            .await
            .is_ok()
    } else {
        false
    };

    // Check if we need to download based on JAR and assets availability
    let assets_exist_in_server = local_assets_path.exists();
    // Re-check if assets exist in client since source_candidate was consumed
    let assets_exist_in_client = {
        let specific = main_app_dir.join(&config.branch).join(&best_version);
        if specific.exists() && specific.join("Assets.zip").exists() {
            true
        } else if best_version == "latest" {
            let latest = main_app_dir.join(&config.branch).join("latest");
            latest.exists() && latest.join("Assets.zip").exists()
        } else {
            false
        }
    };

    // Only download if NEITHER jar NOR original exists AND no assets are available anywhere
    if !jar_available && (!assets_exist_in_server && !assets_exist_in_client) {
        println!(
            "Downloading server version {} (no JAR or assets found locally)...",
            target_ver_num
        );

        let cache_dir = root_dir.join("cache");
        let _ = tokio::fs::create_dir_all(&cache_dir).await;

        // Manual PWR download
        let pwr_name = format!("0-{}.pwr", target_ver_num);
        let pwr_path = cache_dir.join(&pwr_name);

        if !pwr_path.exists() {
            let os = std::env::consts::OS;
            let arch = "amd64";
            let url = format!(
                "https://game-patches.hytale.com/patches/{}/{}/{}/0/{}.pwr",
                os, arch, config.branch, target_ver_num
            );
            println!("Downloading Patch: {}", url);
            crate::game::downloader::download_file(&client, &url, &pwr_path, |_, _| {}, None)
                .await?;
        }

        println!("Applying patch via Butler...");
        // FIX: Pasamos root_dir para que encuentre Butler en ./tools
        crate::game::patcher::apply_pwr(
            &root_dir,
            &config.branch,
            &pwr_path,
            &version_dir_name,
            &callback,
        )
        .await?;

        // After patch application, update server args to use the extracted assets
        config.server_args =
            generate_server_args_with_direct_assets(&config.server_args, &local_assets_path);
    } else {
        if jar_available {
            println!("[Skip] Base JAR is already available locally. Proceeding to patch check.");
        } else {
            println!(
                "[Skip] Assets are available locally (server: {}, client: {}). Proceeding with existing files.",
                assets_exist_in_server, assets_exist_in_client
            );
        }
    }

    // 4. Preparar Directorio de Ejecucion (Ensure vanilla state)
    let server_dir = server_jar_raw.parent().unwrap();
    if let Err(e) = crate::game::patcher::ensure_vanilla_jar(server_dir) {
        eprintln!("[Runner] Warning: Failed to ensure vanilla jar: {}", e);
    }
    let target_jar_path = server_jar_raw.clone();
    println!("Using vanilla JAR: {:?}", target_jar_path);

    // 5. Preparar Entorno de Ejecucion
    println!("[4/5] Preparing Runtime...");

    let java_exec = crate::java::get_java_exec(&root_dir)?;
    let java_path = PathBuf::from(&java_exec);

    // --- PROXY SETUP (SERVER) ---
    // Ensure the proxy is active and updated
    let final_java = match crate::game::patcher::setup_java_proxy(&java_path) {
        Ok(p) => {
            println!("[Runner] (Server) Java Proxy updated and active.");
            p.to_string_lossy().to_string()
        }
        Err(e) => {
            eprintln!("[Runner] (Server) Failed to setup Java Proxy: {}", e);
            java_exec.clone()
        }
    };

    crate::util::make_executable(&PathBuf::from(&final_java)).await?;

    if let Some(provider) = &config.tunnel_provider {
        if provider == "playit" {
            let root_clone = root_dir.clone();
            let client_clone = client.clone();

            let (tx, mut rx) = mpsc::channel(1);

            println!("Starting tunnel and waiting for connection details...");

            tokio::spawn(async move {
                if let Err(e) =
                    crate::server::tunnel::start_playit(&root_clone, &client_clone, tx).await
                {
                    eprintln!("Tunnel Error: {}", e);
                }
            });

            // BLOQUEO: Esperamos aqui hasta que Playit diga algo
            let timeout = tokio::time::sleep(tokio::time::Duration::from_secs(60));

            tokio::select! {
                res = rx.recv() => {
                    match res {
                        Some(msg) => {
                            println!("\n---------------------------------------------------");
                            println!("TUNNEL STATUS: {}", msg);
                            println!("---------------------------------------------------\n");
                            // Aqui podrias parsear la IP si la mandaste en el msg
                        },
                        None => println!("Tunnel closed unexpectedly."),
                    }
                }
                _ = timeout => {
                    println!("WARNING: Tunnel took too long to negotiate. Starting server anyway.");
                }
            }
        }
    }

    // 6. Ejecutar
    println!("[5/5] Launching Server on port 5520!");
    println!("---------------------------------------------------");
    println!("Press Ctrl+C to stop the server safely.");

    // Final validation before launch
    println!("Final pre-launch validation:");
    println!("  - Working directory: {:?}", install_dir);
    println!("  - Server JAR: {:?}", target_jar_path);

    if !target_jar_path.exists() {
        eprintln!("FATAL: Patched JAR not found at {:?}", target_jar_path);
        return Err(anyhow::anyhow!(
            "Patched JAR missing - this will cause Exit Code 7"
        ));
    }

    let assets_in_install_dir = install_dir.join("Assets.zip");
    if assets_in_install_dir.exists() {
        println!("  - Assets.zip: {:?} (local copy)", assets_in_install_dir);
    } else {
        println!("  - Assets.zip: Using direct assets path");
    }

    let server_dir = install_dir.join("Server");
    if !server_dir.exists() {
        eprintln!("WARNING: Server directory not found at {:?}", server_dir);
    }

    // DEBUG: Show final server arguments
    println!("  - Final server args: {}", config.server_args);

    let mut cmd = Command::new(final_java);

    // Inject AURORA_MODE so the Proxy knows what to do
    cmd.env("AURORA_MODE", &config.online_mode);

    cmd.current_dir(&install_dir);

    // Filter out AOT args explicitly to avoid errors
    let java_args: Vec<&str> = config
        .java_exec_args
        .split_whitespace()
        .filter(|a| !a.starts_with("-XX:AOTCache"))
        .collect();

    // Inject DUAL AUTH CONFIG
    if let Some(domain) = &config.auth_domain {
        // If it's the "pseudo-local" one but we have a real dynamic port, update it
        if domain == "127.0.0.000001" {
            cmd.env(
                "HYTALE_AUTH_DOMAIN",
                format!("127.0.0.000001:{}", auth_port),
            );
        } else {
            cmd.env("HYTALE_AUTH_DOMAIN", domain);
        }
    } else {
        // Fallback safety
        if config.online_mode == "local" {
            cmd.env(
                "HYTALE_AUTH_DOMAIN",
                format!("127.0.0.000001:{}", auth_port),
            );
        } else if config.online_mode == "sanasol" {
            cmd.env("HYTALE_AUTH_DOMAIN", "sessions.sanasol.ws");
        }
    }

    // Disable Sentry for server
    cmd.env("DISABLE_SENTRY", "1");

    // Inject Omni-Auth and Trusted Issuers
    cmd.env(
        "HYTALE_TRUST_ALL_ISSUERS",
        config.trust_all_issuers.to_string(),
    );
    if !config.trusted_issuers.is_empty() {
        cmd.env("HYTALE_TRUSTED_ISSUERS", config.trusted_issuers.join(","));
    }

    // Inject Java Agent
    let agent_path = crate::game::paths::GamePaths::new(root_dir.clone()).dualauth_agent();
    if agent_path.exists() {
        cmd.arg(format!("-javaagent:{}", agent_path.to_string_lossy()));
    } else {
        println!(
            "  - WARNING: Java Agent NOT FOUND at {:?}. Authentication may fail.",
            agent_path
        );
    }

    cmd.args(java_args).arg("-jar").arg(&target_jar_path);
    cmd.args(config.server_args.split_whitespace());
    cmd.arg("--disable-sentry");

    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdin(std::process::Stdio::inherit());

    cmd.env("RUSTALE_IS_SERVER", "1");

    // Ensure no console window appears even for server (as we capture logs)
    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000);
    }

    cmd.kill_on_drop(true);

    let mut child = cmd.spawn().context("Failed to spawn java process")?;

    // --- NUEVO: Captura manual de logs ---
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    use tokio::io::{AsyncBufReadExt, BufReader};

    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            println!("[Server] {}", line);
        }
    });

    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            eprintln!("[Server-Err] {}", line);
        }
    });
    // ---------------------------------------

    #[cfg(windows)]
    let _job_guard = {
        use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_ALL_ACCESS};

        match crate::util::win_job::JobObject::new() {
            Ok(job) => {
                if let Some(pid) = child.id() {
                    unsafe {
                        let process_handle = OpenProcess(PROCESS_ALL_ACCESS, 0, pid);

                        if process_handle.is_null() {
                            eprintln!("[Warning] Failed to open process handle for PID {}", pid);
                            None
                        } else {
                            if let Err(e) = job.add_process(process_handle) {
                                eprintln!("[Warning] Failed to assign Java to Job Object: {}", e);
                                windows_sys::Win32::Foundation::CloseHandle(process_handle);
                                None
                            } else {
                                println!(
                                    "[Security] Process attached to Job Object. Java will terminate if launcher closes."
                                );
                                windows_sys::Win32::Foundation::CloseHandle(process_handle);
                                Some(job)
                            }
                        }
                    }
                } else {
                    eprintln!("[Warning] Could not get child process PID");
                    None
                }
            }
            Err(e) => {
                eprintln!("[Warning] Could not create Job Object: {}", e);
                None
            }
        }
    };

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            println!("\n[RusTale] Stop signal received. Closing server...");

            let _ = child.kill().await;
        }
        status = child.wait() => {
            println!("[RusTale] Server process terminated: {:?}", status?);
        }
    }

    Ok(())
}
