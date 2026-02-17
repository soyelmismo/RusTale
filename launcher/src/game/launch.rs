use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};
use tokio::process::{Child, Command};

/// Context structure for game launch operations
/// This encapsulates all parameters needed to launch the game cleanly
#[derive(Debug, Clone)]
pub struct LaunchContext {
    pub player_name: String,
    pub player_uuid: String,
    pub exec_path: PathBuf,
    pub working_dir: PathBuf,
    pub user_data_dir: PathBuf,
    pub java_path: String,
    pub auth_args: Vec<String>,
    pub env_vars: std::collections::HashMap<String, String>,
}

/// Launches the Hytale game client with async agent download
/// This function launches the game immediately and downloads the dualauth-agent in background
pub fn launch_game_with_async_agent(
    ctx: LaunchContext,
    client: reqwest::Client,
) -> Result<Child> {
    launch_game_with_agent(ctx, Some(client))
}

/// Launches the Hytale game client with optional async agent download
/// Internal function that handles both the game launch and optional agent download
fn launch_game_with_agent(
    ctx: LaunchContext,
    client: Option<reqwest::Client>,
) -> Result<Child> {
    // SANITIZACION DE RUTAS:
    // Java y algunas librerías nativas fallan con rutas relativas o con rutas UNC de Windows (\\?\C:\\...).
    // Convertimos todo a absoluto limpio.
    let clean_exec = crate::util::sanitize_path(&ctx.exec_path);
    let clean_work_dir = crate::util::sanitize_path(&ctx.working_dir);
    let clean_user_dir = crate::util::sanitize_path(&ctx.user_data_dir);

    // Logs para debugging de rutas (muy útil para reportes de usuario)
    println!("[Launch] Path Sanitation:");
    println!("  > Executable: {:?}", clean_exec);
    println!("  > Work Dir:   {:?}", clean_work_dir);
    println!("  > User Data:  {:?}", clean_user_dir);

    // Ensure user data directory exists
    std::fs::create_dir_all(&clean_user_dir).context("Failed to create UserData directory")?;

    // Verify client exists (final safety check)
    if !clean_exec.exists() {
        anyhow::bail!("Game executable not found at: {}", clean_exec.display());
    }

    println!(
        "Launching {} with UUID {} from {}",
        ctx.player_name,
        ctx.player_uuid,
        clean_exec.display()
    );

    // Build command
    let mut cmd = build_game_command(ctx);

    // Configure Linux-specific environment
    #[cfg(target_os = "linux")]
    {
        let exec_path_for_linux = PathBuf::from(cmd.as_std().get_program());
        let working_dir_for_linux = clean_work_dir.clone();
        configure_linux_env(&mut cmd, &exec_path_for_linux, &working_dir_for_linux)?;
    }

    cmd.kill_on_drop(true);

    // Explicitamente setear el Current Directory al del juego para asegurar consistencia
    cmd.current_dir(&clean_work_dir);

    // Spawn the game process
    let child = cmd.spawn().context("Failed to start game process")?;

    spawn_agent_download(client);

    Ok(child)
}

/// Build base game command with common arguments
fn build_game_command(ctx: LaunchContext) -> Command {
    let mut cmd = Command::new(&ctx.exec_path);

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000);
    }

    cmd.arg("--app-dir")
        .arg(&ctx.working_dir)
        .arg("--user-dir")
        .arg(&ctx.user_data_dir)
        .arg("--java-exec")
        .arg(&ctx.java_path)
        .arg("--uuid")
        .arg(&ctx.player_uuid)
        .arg("--name")
        .arg(&ctx.player_name);

    for arg in ctx.auth_args {
        cmd.arg(arg);
    }

    for (key, value) in ctx.env_vars {
        cmd.env(key, value);
    }

    cmd
}

/// Configure Linux-specific environment variables for game command
#[cfg(target_os = "linux")]
fn configure_linux_env(
    cmd: &mut Command,
    exec_path: &PathBuf,
    working_dir: &PathBuf,
) -> Result<()> {
    {
        // 1. SDL Video Driver (Wayland/X11)
        if is_wayland() {
            cmd.env("SDL_VIDEODRIVER", "wayland");
        }

        if let Some(parent) = exec_path.parent() {
            let current_ld_path = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();

            // Usar rutas absolutas aquí también es crítico para Linux
            let parent_abs = parent.canonicalize().unwrap_or(parent.to_path_buf());
            let work_abs = working_dir
                .canonicalize()
                .unwrap_or(working_dir.to_path_buf());

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
            )
            .await
            {
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
