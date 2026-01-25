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
    // Necesitamos un canal para detener el servidor web cuando el proceso termine (aunque al matar el proceso muere todo)
    let (_server_stop_tx, server_stop_rx) = tokio::sync::oneshot::channel::<()>();

    if config.online_mode == "local" {
        // 1. Comprobar si ya existe
        if crate::game::server::is_server_alive(auth_port).await {
            println!(
                "Local Auth Server already running on port {}. Attaching...",
                auth_port
            );
            // No hacemos nada, simplemente usamos el puerto.
        } else {
            println!(
                "Starting Local Auth Server (Emulator) on port {}...",
                auth_port
            );

            // Generar credenciales "Host" dummy para el servidor dedicado
            let host_uuid = uuid::Uuid::new_v4().to_string();
            let host_name = "ConsoleHost".to_string();

            // El directorio de assets para skins suele ser el del juego instalado
            let version_dir_name = if config.game_version == "latest" || config.game_version == "0"
            {
                "latest".to_string()
            } else {
                config.game_version.clone()
            };
            let install_dir_ref = root_dir.join(&config.branch).join(&version_dir_name);

            // Lanzar el servidor web en background
            tokio::spawn(async move {
                if let Err(e) = crate::game::server::start_server(
                    host_name,
                    host_uuid,
                    install_dir_ref,
                    server_stop_rx,
                    auth_port,
                )
                .await
                {
                    eprintln!("Auth Server Error: {}", e);
                }
            });

            // Esperar un momento para asegurar que arranque
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
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

    // 2. Asegurar Herramientas (JRE y Butler)
    println!("[1/5] Checking tools...");
    let callback = |task: &str, pct: f64, msg: &str| {
        if pct == 0.0 || pct == 100.0 {
            println!("[{}] {}", task, msg);
        }
    };

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

    let mut cmd = Command::new(java_exec);

    cmd.current_dir(&install_dir)
        .args(config.java_exec_args.split_whitespace())
        .arg("-jar")
        .arg(&patched_jar_path);
    cmd.args(config.server_args.split_whitespace());

    cmd.stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .stdin(std::process::Stdio::inherit());

    let mut child = cmd.spawn().context("Failed to spawn java process")?;

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            println!("\nStopping server...");
            let _ = child.kill().await;
        }
        status = child.wait() => {
            println!("Server exited with: {:?}", status?);
        }
    }

    // LIMPIEZA: Matar el túnel al salir
    // LIMPIEZA: El túnel se cerrará automáticamente al finalizar el runtime gracias a kill_on_drop
    // if let Some(mut tp) = tunnel_process {
    //     println!("Stopping Tunnel...");
    //     let _ = tp.kill().await;
    // }

    Ok(())
}
