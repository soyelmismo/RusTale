use crate::java::tracking::track_process;
use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::process::{Child, Command};

/// Context structure for game launch operations
/// This encapsulates all parameters needed to launch game cleanly
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
) -> Result<Child> {
    launch_game_with_agent(ctx)
}

/// Launches the Hytale game client with optional async agent download
/// Internal function that handles both game launch and optional agent download
pub fn launch_game_with_agent(
    ctx: LaunchContext,
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

    // Guardar exec_path antes de mover ctx
    let exec_path_for_name = ctx.exec_path.clone();

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
    
    // Trackear el PID del proceso del juego
    if let Some(pid) = child.id() {
        let process_name = if exec_path_for_name.to_string_lossy().contains("HytaleClient") {
            "HytaleClient"        // ← Nombre real del ejecutable
        } else if exec_path_for_name.to_string_lossy().contains("HytaleServer") {
            "HytaleServer"        // ← Nombre real del ejecutable
        } else {
            "game_process"
        };
        track_process(pid, process_name.to_string());
    }

    // The agent is now downloaded synchronously before proxy setup in launcher.rs
    // This ensures it's available before any Java process starts

    Ok(child)
}

/// Build native game client command with common arguments.
///
/// Hytale client is always a native binary (HytaleClient.exe on Windows,
/// HytaleClient on Linux/macOS). Java is only used for the server component.
pub fn build_game_command(ctx: LaunchContext) -> Command {
    // Native binary: execute directly (e.g. HytaleClient on Linux, HytaleClient.exe on Windows)
    #[allow(unused_mut)]
    let mut cmd = Command::new(&ctx.exec_path);

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000);
    }

    // Common game arguments for the native client
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
pub fn configure_linux_env(
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

#[cfg(target_os = "linux")]
pub fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok()
        || std::env::var("XDG_SESSION_TYPE")
            .map(|s| s == "wayland")
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_native_client_launch() {
        // Document: The client is always a native binary, never a JAR
        
        // Native client should be executed directly
        let native_path = PathBuf::from("/game/HytaleClient");
        let ctx = LaunchContext {
            player_name: "Test".to_string(),
            player_uuid: "uuid".to_string(),
            exec_path: native_path.clone(),
            working_dir: PathBuf::from("/game"),
            user_data_dir: PathBuf::from("/game/UserData"),
            java_path: "java".to_string(),
            auth_args: vec![],
            env_vars: std::collections::HashMap::new(),
        };
        let cmd = build_game_command(ctx);
        
        let program = cmd.as_std().get_program().to_string_lossy().to_string();
        assert!(program.contains("HytaleClient"), "Native client should be executed directly");
        assert_ne!(program, "java", "Native client should NOT use java");
    }
}
