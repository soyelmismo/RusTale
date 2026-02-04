use anyhow::{Context, Result};
use std::path::PathBuf;
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
    // Ensure user data directory exists
    std::fs::create_dir_all(user_data_dir).context("Failed to create UserData directory")?;

    // Verify client exists (final safety check)
    if !executable_path.exists() {
        anyhow::bail!(
            "Game executable not found at: {}",
            executable_path.display()
        );
    }

    println!(
        "Launching {} with UUID {} from {}",
        player_name,
        player_uuid,
        executable_path.display()
    );

    // Build command
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

    // Inject dynamic auth args
    for arg in extra_auth_args {
        cmd.arg(arg);
    }

    // INYECTAR VARIABLES DE ENTORNO
    for (key, value) in env_vars {
        cmd.env(key, value);
    }

    #[cfg(target_os = "linux")]
    {
        // 1. SDL Video Driver (Wayland/X11)
        if is_wayland() {
            cmd.env("SDL_VIDEODRIVER", "wayland");
        }

        if let Some(parent) = executable_path.parent() {
            let current_ld_path = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();

            let new_ld_path = format!(
                "{}:{}:{}",
                parent.to_string_lossy(),
                game_working_dir.to_string_lossy(),
                current_ld_path
            );

            cmd.env("LD_LIBRARY_PATH", new_ld_path);
            println!("[Linux] Injected LD_LIBRARY_PATH fixes.");
        }
    }

    cmd.kill_on_drop(true);
    
    // Spawn the game process
    let child = cmd.spawn()
        .context("Failed to start game process")?;

    // --- ASYNC AGENT DOWNLOAD ---
    // If we have a client, start the agent download in background after game launch
    if let Some(http_client) = client {
        let base_dir = crate::config::get_app_dir();
        tokio::spawn(async move {
            // Small delay to ensure game starts first
            tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
            
            println!("[Agent] Starting background download after game launch...");
            
            // Use a simple progress callback that just logs
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
            } else {
                println!("[Agent] Background download completed successfully");
            }
        });
    }

    Ok(child)
}
#[cfg(target_os = "linux")]
fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok()
        || std::env::var("XDG_SESSION_TYPE")
            .map(|s| s == "wayland")
            .unwrap_or(false)
}
