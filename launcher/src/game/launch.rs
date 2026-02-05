use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::process::{Child, Command};
use std::sync::{Arc, atomic::AtomicBool};

/// Launches the Hytale game client with async agent download
/// This function launches the game immediately and downloads the dualauth-agent in background
pub fn launch_game_with_async_agent(
    player_name: &str,
    player_uuid: &str,
    executable_path: &PathBuf,  // Absolute path to HytaleClient.exe
    game_working_dir: &PathBuf, // Absolute path to the version directory
    user_data_dir: &PathBuf,    // Absolute path to UserData directory
    java_exec: &str,            // Path to java executable
    extra_auth_args: Vec<String>,
    env_vars: std::collections::HashMap<String, String>,
    client: reqwest::Client,
) -> Result<Child> {
    launch_game_with_agent(player_name, player_uuid, executable_path, game_working_dir, user_data_dir, java_exec, extra_auth_args, env_vars, Some(client))
}

/// Launches the Hytale game client asynchronously
/// This function is "dumb" - it only executes the game with the provided paths.
/// All verification and path resolution should be done by the caller.
pub fn launch_game(
    player_name: &str,
    player_uuid: &str,
    executable_path: &PathBuf,  // Absolute path to HytaleClient.exe
    game_working_dir: &PathBuf, // Absolute path to the version directory
    user_data_dir: &PathBuf,    // Absolute path to UserData directory
    java_exec: &str,            // Path to java executable
    extra_auth_args: Vec<String>,
    env_vars: std::collections::HashMap<String, String>,
) -> Result<Child> {
    launch_game_with_agent(player_name, player_uuid, executable_path, game_working_dir, user_data_dir, java_exec, extra_auth_args, env_vars, None)
}

/// Launches the Hytale game client with optional async agent download
/// Internal function that handles both the game launch and optional agent download
fn launch_game_with_agent(
    player_name: &str,
    player_uuid: &str,
    executable_path: &PathBuf,  // Absolute path to HytaleClient.exe
    game_working_dir: &PathBuf, // Absolute path to the version directory
    user_data_dir: &PathBuf,    // Absolute path to UserData directory
    java_exec: &str,            // Path to java executable
    extra_auth_args: Vec<String>,
    env_vars: std::collections::HashMap<String, String>,
    client: Option<reqwest::Client>,
) -> Result<Child> {
    // SANITIZACION DE RUTAS: 
    // Java y algunas librerías nativas fallan con rutas relativas o con rutas UNC de Windows (\\\\?\\C:\\\\...).
    // Convertimos todo a absoluto limpio.
    let clean_exec = crate::util::sanitize_path(executable_path);
    let clean_work_dir = crate::util::sanitize_path(game_working_dir);
    let clean_user_dir = crate::util::sanitize_path(user_data_dir);

    // Logs para debugging de rutas (muy útil para reportes de usuario)
    println!("[Launch] Path Sanitation:");
    println!("  > Executable: {:?}", clean_exec);
    println!("  > Work Dir:   {:?}", clean_work_dir);
    println!("  > User Data:  {:?}", clean_user_dir);

    // Ensure user data directory exists
    std::fs::create_dir_all(&clean_user_dir).context("Failed to create UserData directory")?;

    // Verify client exists (final safety check)
    if !clean_exec.exists() {
        anyhow::bail!(
            "Game executable not found at: {}",
            clean_exec.display()
        );
    }

    println!(
        "Launching {} with UUID {} from {}",
        player_name,
        player_uuid,
        clean_exec.display()
    );

    // Build command
    let mut cmd = build_game_command(
        &clean_exec,
        &clean_work_dir,
        &clean_user_dir,
        java_exec,
        player_uuid,
        player_name,
        extra_auth_args,
        env_vars,
    );

    // Configure Linux-specific environment
    configure_linux_env(&mut cmd, &clean_exec, &clean_work_dir)?;

    cmd.kill_on_drop(true);
    
    // Explicitamente setear el Current Directory al del juego para asegurar consistencia
    cmd.current_dir(&clean_work_dir);
    
    // Spawn the game process
    let child = cmd.spawn()
        .context("Failed to start game process")?;

    spawn_agent_download(client);

    Ok(child)
}

/// Build base game command with common arguments
fn build_game_command(
    executable_path: &PathBuf,
    game_working_dir: &PathBuf,
    user_data_dir: &PathBuf,
    java_exec: &str,
    player_uuid: &str,
    player_name: &str,
    extra_auth_args: Vec<String>,
    env_vars: std::collections::HashMap<String, String>,
) -> Command {
    let mut cmd = Command::new(executable_path);

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000);
    }
    
    cmd.arg("--app-dir")
        .arg(game_working_dir)
        .arg("--user-dir")
        .arg(user_data_dir)
        .arg("--java-exec")
        .arg(java_exec)
        .arg("--uuid")
        .arg(player_uuid)
        .arg("--name")
        .arg(player_name);

    for arg in extra_auth_args {
        cmd.arg(arg);
    }

    for (key, value) in env_vars {
        cmd.env(key, value);
    }

    cmd
}

/// Configure Linux-specific environment variables for game command
fn configure_linux_env(cmd: &mut Command, executable_path: &PathBuf, game_working_dir: &PathBuf) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        // 1. SDL Video Driver (Wayland/X11)
        if is_wayland() {
            cmd.env("SDL_VIDEODRIVER", "wayland");
        }

        if let Some(parent) = executable_path.parent() {
            let current_ld_path = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();

            // Usar rutas absolutas aquí también es crítico para Linux
            let parent_abs = parent.canonicalize().unwrap_or(parent.to_path_buf());
            let work_abs = game_working_dir.canonicalize().unwrap_or(game_working_dir.to_path_buf());

            let new_ld_path = format!(
                "{}:{}:{}",
                parent_abs.to_string_lossy(),
                work_abs.to_string_lossy(),
                current_ld_path
            );

            cmd.env("LD_LIBRARY_PATH", new_ld_path);
            println!("[Linux] Injected LD_LIBRARY_PATH fixes.");
        }
    }
    
    Ok(())
}

fn spawn_agent_download(client: Option<reqwest::Client>) {
    if let Some(http_client) = client {
        let base_dir = crate::config::get_app_dir();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
            let progress_callback = |phase: &str, progress: f64, msg: &str| {
                println!("[Agent] {} {:.1}% - {}", phase, progress, msg);
            };
            if let Err(e) = crate::game::agent::ensure_agent(
                &http_client,
                &base_dir,
                &progress_callback,
                None::<Arc<AtomicBool>>,
            ).await {
                eprintln!("[Agent] Background download failed: {}", e);
            }
        });
    }
}

#[cfg(target_os = "linux")]
fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok()
        || std::env::var("XDG_SESSION_TYPE")
            .map(|s| s == "wayland")
            .unwrap_or(false)
}
