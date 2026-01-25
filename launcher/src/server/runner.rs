use crate::config::OnlineFixMode;
use crate::server::config::ServerConfig;
use anyhow::{Context, Result};
use rand::Rng;
use std::path::PathBuf;
use tokio::process::Command;
use tokio::sync::mpsc;

pub async fn run_server_flow(config: ServerConfig) -> Result<()> {
    println!("--- RusTale Dedicated Server ---");
    println!(
        "Mode: {} | Port: 5520 | Version: {}",
        config.online_mode, config.game_version
    );

    // 1. Definir Rutas (Unificadas a root_dir para evitar OS Error 3)
    let root_dir = crate::config::get_server_root_dir();
    let _ = tokio::fs::create_dir_all(&root_dir).await;
    let port_file = root_dir.join("server.port");

    let auth_port: u16 = if port_file.exists() {
        // Si ya existe un puerto guardado, lo usamos para no tener que reparchear el JAR
        let content = tokio::fs::read_to_string(&port_file)
            .await
            .unwrap_or_default();
        content.trim().parse().unwrap_or_else(|_| {
            println!("Corrupt port file, generating new one...");
            0
        })
    } else {
        0
    };

    let auth_port = if auth_port > 0 {
        auth_port
    } else {
        // Buscar puerto libre aleatorio (10000 - 60000)
        let mut rng = rand::rng();
        let new_port = rng.random_range(10000..60000);

        // Guardarlo para el futuro
        if let Err(e) = tokio::fs::write(&port_file, new_port.to_string()).await {
            eprintln!("Warning: Could not save auth port to file: {}", e);
        }
        println!("Allocated new Auth Port: {}", new_port);
        new_port
    };

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
        eprintln!("Please close 'java.exe' or 'rustale.exe' from Task Manager.\n");
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
        .user_agent("RusTale-Server/0.0.1")
        .build()?;

    // 2. Tools (JRE, Butler)
    println!("[1/5] Checking tools...");
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

    // 3. Resolver Versión y Descargar Servidor
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

        // Candidate 1: Specific version folder
        let specific = main_app_dir
            .join(&config.branch)
            .join(target_ver_num.to_string());
        if specific.exists() && specific.join("Server").exists() {
            source_candidate = Some(specific);
        }

        // Candidate 2: Latest folder (check version.json)
        if source_candidate.is_none() {
            let latest = main_app_dir.join(&config.branch).join("latest");
            if latest.exists() {
                if let Ok(ver) =
                    crate::game::install::get_local_version(&main_app_dir, &config.branch).await
                {
                    if ver == target_ver_num {
                        source_candidate = Some(latest);
                    }
                }
            }
        }

        if let Some(src) = source_candidate {
            println!(
                "Found matching version {} at {:?}. Copying files...",
                target_ver_num, src
            );
            let _ = tokio::fs::create_dir_all(&install_dir).await;

            // Copy Assets.zip
            let src_assets = src.join("Assets.zip");
            let dst_assets = install_dir.join("Assets.zip");
            if src_assets.exists() && !dst_assets.exists() {
                println!("Copying Assets.zip (~3GB)...");
                if let Err(e) = tokio::fs::copy(&src_assets, &dst_assets).await {
                    eprintln!("Failed to copy Assets.zip: {}", e);
                }
            }

            // Copy Server folder
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

    if !server_jar_raw.exists() {
        println!("Downloading server version {}...", target_ver_num);

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
    }

    // 4. Parchear el JAR para Online Mode
    println!("[3/5] Patching JAR for {} mode...", config.online_mode);

    let mode_enum = match config.online_mode.as_str() {
        "sanasol" => OnlineFixMode::Sanasol,
        _ => OnlineFixMode::Local,
    };

    let patched_jar_name = format!(
        "HytaleServer.{}.{}.{}.jar",
        config.online_mode, config.branch, target_ver_num
    );
    let patched_jar_path = install_dir.join("Server").join(&patched_jar_name);

    if !patched_jar_path.exists() {
        println!("Generating patched JAR: {}", patched_jar_name);
        crate::game::patcher::patch_server_jar(
            &server_jar_raw,
            &patched_jar_path,
            mode_enum,
            auth_port,
        )?;
    } else {
        println!("Using existing patched JAR: {}", patched_jar_name);
    }

    // 5. Preparar Entorno de Ejecución
    println!("[4/5] Preparing Runtime...");

    let java_exec = crate::java::get_java_exec(&root_dir)?;

    crate::util::make_executable(&PathBuf::from(&java_exec)).await?;

    if let Some(provider) = &config.tunnel_provider {
        if provider == "playit" {
            let root_clone = root_dir.clone();
            let client_clone = client.clone();

            // Creamos un canal para esperar la señal
            let (tx, mut rx) = mpsc::channel(1);

            println!("Starting tunnel and waiting for connection details...");

            // Lanzamos el túnel
            tokio::spawn(async move {
                if let Err(e) =
                    crate::server::tunnel::start_playit(&root_clone, &client_clone, tx).await
                {
                    eprintln!("Tunnel Error: {}", e);
                }
            });

            // BLOQUEO: Esperamos aquí hasta que Playit diga algo
            // Añadimos un timeout de 60 segundos para no bloquearnos eternamente
            let timeout = tokio::time::sleep(tokio::time::Duration::from_secs(60));

            tokio::select! {
                res = rx.recv() => {
                    match res {
                        Some(msg) => {
                            println!("\n---------------------------------------------------");
                            println!("TUNNEL STATUS: {}", msg);
                            println!("---------------------------------------------------\n");
                            // Aquí podrías parsear la IP si la mandaste en el msg
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

    let mut cmd = Command::new(java_exec);

    cmd.current_dir(&install_dir)
        .args(config.java_exec_args.split_whitespace())
        .arg("-jar")
        .arg(&patched_jar_path);
    cmd.args(config.server_args.split_whitespace());

    cmd.stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .stdin(std::process::Stdio::inherit());

    cmd.kill_on_drop(true);

    let mut child = cmd.spawn().context("Failed to spawn java process")?;

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
