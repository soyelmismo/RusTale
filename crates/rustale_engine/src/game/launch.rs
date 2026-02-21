use crate::java::tracking::track_process;
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};
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
    pub jvm_args: Option<Vec<String>>,
}

/// Launches the Hytale game client with async agent download
/// This function launches the game immediately and downloads the dualauth-agent in background
pub fn launch_game_with_async_agent(
    ctx: LaunchContext,
    client: rustale_shared::reqwest::Client,
) -> Result<Child> {
    launch_game_with_agent(ctx, Some(client))
}

/// Launches the Hytale game client with optional async agent download
/// Internal function that handles both game launch and optional agent download
pub fn launch_game_with_agent(
    ctx: LaunchContext,
    client: Option<rustale_shared::reqwest::Client>,
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

    spawn_agent_download(client);

    Ok(child)
}

/// Build base game command with common arguments.
///
/// Hytale uses a native binary on Linux (`HytaleClient`) and a JAR on other
/// platforms (`HytaleServer.jar`, `HytaleClient.jar`).  We distinguish the two
/// cases by checking the file extension:
///   - `.jar` → launch via `java -jar <path>` (with JVM args)
///   - anything else → launch the binary directly (no java, no JVM args)
pub fn build_game_command(ctx: LaunchContext) -> Command {
    let is_jar = ctx
        .exec_path
        .extension()
        .map(|ext| ext.eq_ignore_ascii_case("jar"))
        .unwrap_or(false);

    let mut cmd = if is_jar {
        // JAR: run through JVM
        let mut c = Command::new(&ctx.java_path);

        #[cfg(target_os = "windows")]
        {
            c.creation_flags(0x08000000);
        }

        // Add JVM args if provided
        if let Some(jvm_args) = &ctx.jvm_args {
            for arg in jvm_args {
                c.arg(arg);
            }
        }

        c.arg("-jar").arg(&ctx.exec_path);
        c
    } else {
        // Native binary: execute directly (e.g. HytaleClient on Linux)
        println!("[Launch] Native binary detected — skipping java -jar.");
        // `mut` is only used on Windows for creation_flags; suppress the lint on other platforms.
        #[allow(unused_mut)]
        let mut c = Command::new(&ctx.exec_path);
        #[cfg(target_os = "windows")]
        c.creation_flags(0x08000000);
        c
    };

    // Common game arguments (both JAR and native understand these)
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

fn spawn_agent_download(client: Option<rustale_shared::reqwest::Client>) {
    if let Some(http_client) = client {
        let base_dir = rustale_shared::config::get_app_dir();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
            let progress_callback = |phase: String, progress: f64, msg: String| {
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
pub fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok()
        || std::env::var("XDG_SESSION_TYPE")
            .map(|s| s == "wayland")
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    // === Cross-Platform Launch Behavior Tests ===
    // These tests document expected behavior differences between platforms

    #[test]
    fn test_jar_detection_logic() {
        // Document: .jar files are launched via JVM, others are native binaries
        
        // JAR files should use Java
        let jar_path = PathBuf::from("/game/HytaleClient.jar");
        let is_jar = jar_path.extension()
            .map(|ext| ext.eq_ignore_ascii_case("jar"))
            .unwrap_or(false);
        assert!(is_jar, ".jar extension should be detected");
        
        // Native binaries should NOT use Java
        let native_path = PathBuf::from("/game/HytaleClient");
        let is_jar_native = native_path.extension()
            .map(|ext| ext.eq_ignore_ascii_case("jar"))
            .unwrap_or(false);
        assert!(!is_jar_native, "No extension should not be detected as JAR");
        
        // Windows executable should NOT use Java
        let exe_path = PathBuf::from("C:\\game\\HytaleClient.exe");
        let is_jar_exe = exe_path.extension()
            .map(|ext| ext.eq_ignore_ascii_case("jar"))
            .unwrap_or(false);
        assert!(!is_jar_exe, ".exe extension should not be detected as JAR");
    }

    #[test]
    fn test_platform_specific_launch_mode() {
        // Document the expected launch mode per platform:
        // - Windows: Usually JAR via java.exe
        // - Linux: Native binary HytaleClient OR JAR via java
        // - macOS: Usually JAR via java
        
        // This test documents that we handle BOTH cases
        let ctx_jar = LaunchContext {
            player_name: "Test".to_string(),
            player_uuid: "uuid".to_string(),
            exec_path: PathBuf::from("/game/HytaleClient.jar"),
            working_dir: PathBuf::from("/game"),
            user_data_dir: PathBuf::from("/game/UserData"),
            java_path: "java".to_string(),
            auth_args: vec![],
            env_vars: std::collections::HashMap::new(),
            jvm_args: None,
        };
        
        let cmd_jar = build_game_command(ctx_jar);
        let program_jar = cmd_jar.as_std().get_program().to_string_lossy().to_string();
        assert_eq!(program_jar, "java", "JAR files should use java command");
        
        let ctx_native = LaunchContext {
            player_name: "Test".to_string(),
            player_uuid: "uuid".to_string(),
            exec_path: PathBuf::from("/game/HytaleClient"),
            working_dir: PathBuf::from("/game"),
            user_data_dir: PathBuf::from("/game/UserData"),
            java_path: "java".to_string(),
            auth_args: vec![],
            env_vars: std::collections::HashMap::new(),
            jvm_args: None,
        };
        
        let cmd_native = build_game_command(ctx_native);
        let program_native = cmd_native.as_std().get_program().to_string_lossy().to_string();
        assert!(program_native.contains("HytaleClient"), 
            "Native binary should be executed directly");
    }

    #[test]
    fn test_java_exec_argument_passed_to_game() {
        // Document: The game receives --java-exec argument so it knows
        // where Java is installed, regardless of platform
        let ctx = LaunchContext {
            player_name: "Test".to_string(),
            player_uuid: "uuid".to_string(),
            exec_path: PathBuf::from("/game/HytaleClient.jar"),
            working_dir: PathBuf::from("/game"),
            user_data_dir: PathBuf::from("/game/UserData"),
            java_path: "/custom/path/to/java".to_string(),
            auth_args: vec![],
            env_vars: std::collections::HashMap::new(),
            jvm_args: None,
        };
        
        let cmd = build_game_command(ctx);
        let args: Vec<String> = cmd.as_std().get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        
        // Find --java-exec and verify its value
        let java_exec_idx = args.iter().position(|a| a == "--java-exec");
        assert!(java_exec_idx.is_some(), "--java-exec should be in args");
        
        let java_exec_value_idx = java_exec_idx.unwrap() + 1;
        assert!(java_exec_value_idx < args.len(), "--java-exec should have a value");
        assert_eq!(args[java_exec_value_idx], "/custom/path/to/java");
    }

    #[test]
    fn test_paths_with_spaces_handled_correctly() {
        // Critical: Windows paths often have spaces (e.g., "C:\\Program Files\\")
        // Command arguments should handle these correctly via the OS
        let ctx = LaunchContext {
            player_name: "Test Player".to_string(),
            player_uuid: "uuid-123".to_string(),
            exec_path: PathBuf::from("/path with spaces/game/HytaleClient.jar"),
            working_dir: PathBuf::from("/path with spaces/game"),
            user_data_dir: PathBuf::from("/path with spaces/UserData"),
            java_path: "/path with spaces/java/bin/java".to_string(),
            auth_args: vec![],
            env_vars: std::collections::HashMap::new(),
            jvm_args: None,
        };
        
        // Should not panic or fail with spaces
        let cmd = build_game_command(ctx);
        let args: Vec<String> = cmd.as_std().get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        
        // Spaces should be preserved in arguments
        assert!(args.iter().any(|a| a.contains("path with spaces")));
    }

    // === Command Building Tests (Snapshot-style) ===

    #[test]
    fn test_build_game_command_jar() {
        let ctx = LaunchContext {
            player_name: "Player1".to_string(),
            player_uuid: "abc-123".to_string(),
            exec_path: PathBuf::from("/game/HytaleClient.jar"),
            working_dir: PathBuf::from("/game"),
            user_data_dir: PathBuf::from("/game/UserData"),
            java_path: "/usr/bin/java".to_string(),
            auth_args: vec!["--online".to_string()],
            env_vars: std::collections::HashMap::new(),
            jvm_args: Some(vec!["-Xmx4G".to_string(), "-Xms1G".to_string()]),
        };

        let cmd = build_game_command(ctx);
        let program = cmd.as_std().get_program().to_string_lossy().to_string();
        
        // Verify Java is used for JAR files
        assert_eq!(program, "/usr/bin/java");
    }

    #[test]
    fn test_build_game_command_native_binary() {
        let ctx = LaunchContext {
            player_name: "Player1".to_string(),
            player_uuid: "abc-123".to_string(),
            exec_path: PathBuf::from("/game/HytaleClient"), // No .jar extension
            working_dir: PathBuf::from("/game"),
            user_data_dir: PathBuf::from("/game/UserData"),
            java_path: "/usr/bin/java".to_string(),
            auth_args: vec![],
            env_vars: std::collections::HashMap::new(),
            jvm_args: None,
        };

        let cmd = build_game_command(ctx);
        let program = cmd.as_std().get_program().to_string_lossy().to_string();
        
        // Native binary should be executed directly, not through java
        assert!(program.contains("HytaleClient"));
        assert_ne!(program, "/usr/bin/java");
    }

    #[test]
    fn test_build_game_command_arguments() {
        let ctx = LaunchContext {
            player_name: "TestUser".to_string(),
            player_uuid: "test-uuid-42".to_string(),
            exec_path: PathBuf::from("/game/client.jar"),
            working_dir: PathBuf::from("/game"),
            user_data_dir: PathBuf::from("/userdata"),
            java_path: "java".to_string(),
            auth_args: vec!["--auth-token".to_string(), "secret123".to_string()],
            env_vars: std::collections::HashMap::new(),
            jvm_args: None,
        };

        let cmd = build_game_command(ctx);
        let args: Vec<String> = cmd
            .as_std()
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect();

        // Verify essential arguments are present
        assert!(args.contains(&"--app-dir".to_string()));
        assert!(args.contains(&"--user-dir".to_string()));
        assert!(args.contains(&"--uuid".to_string()));
        assert!(args.contains(&"--name".to_string()));
        assert!(args.contains(&"--java-exec".to_string()));
        assert!(args.contains(&"test-uuid-42".to_string()));
        assert!(args.contains(&"TestUser".to_string()));
        
        // Verify auth args are appended
        assert!(args.contains(&"--auth-token".to_string()));
        assert!(args.contains(&"secret123".to_string()));
    }

    #[test]
    fn test_build_game_command_jvm_args() {
        let ctx = LaunchContext {
            player_name: "Player".to_string(),
            player_uuid: "uuid".to_string(),
            exec_path: PathBuf::from("/game/client.jar"),
            working_dir: PathBuf::from("/game"),
            user_data_dir: PathBuf::from("/userdata"),
            java_path: "java".to_string(),
            auth_args: vec![],
            env_vars: std::collections::HashMap::new(),
            jvm_args: Some(vec![
                "-Xmx8G".to_string(),
                "-XX:+UseG1GC".to_string(),
                "-Djava.awt.headless=true".to_string(),
            ]),
        };

        let cmd = build_game_command(ctx);
        let args: Vec<String> = cmd
            .as_std()
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect();

        // JVM args should appear before -jar
        let jar_idx = args.iter().position(|a| a == "-jar").expect("Should have -jar");
        
        // Find JVM args positions
        let xmx_idx = args.iter().position(|a| a == "-Xmx8G");
        let g1gc_idx = args.iter().position(|a| a == "-XX:+UseG1GC");
        
        // JVM args should be present and before -jar
        assert!(xmx_idx.is_some());
        assert!(g1gc_idx.is_some());
        assert!(xmx_idx.unwrap() < jar_idx);
        assert!(g1gc_idx.unwrap() < jar_idx);
    }

    #[test]
    fn test_build_game_command_env_vars() {
        let mut env_vars = std::collections::HashMap::new();
        env_vars.insert("HYTALE_ENV".to_string(), "production".to_string());
        env_vars.insert("DEBUG_MODE".to_string(), "false".to_string());

        let ctx = LaunchContext {
            player_name: "Player".to_string(),
            player_uuid: "uuid".to_string(),
            exec_path: PathBuf::from("/game/client.jar"),
            working_dir: PathBuf::from("/game"),
            user_data_dir: PathBuf::from("/userdata"),
            java_path: "java".to_string(),
            auth_args: vec![],
            env_vars,
            jvm_args: None,
        };

        let cmd = build_game_command(ctx);
        let std_cmd = cmd.as_std();
        
        // Check environment variables are set
        // Note: get_envs returns an iterator of (OsString, OsString)
        let envs: std::collections::HashMap<String, String> = std_cmd
            .get_envs()
            .filter_map(|(k, v)| {
                let key = k.to_string_lossy().to_string();
                v.map(|val| (key, val.to_string_lossy().to_string()))
            })
            .collect();

        // The env vars should be set (along with inherited ones)
        assert_eq!(envs.get("HYTALE_ENV"), Some(&"production".to_string()));
        assert_eq!(envs.get("DEBUG_MODE"), Some(&"false".to_string()));
    }

    // === Argument Format Snapshot Test ===
    
    #[test]
    fn test_command_args_snapshot() {
        // This test validates the exact argument format
        // If arguments change, update this test intentionally
        let ctx = LaunchContext {
            player_name: "SnapshotTest".to_string(),
            player_uuid: "snapshot-uuid-001".to_string(),
            exec_path: PathBuf::from("/game/HytaleClient.jar"),
            working_dir: PathBuf::from("/game"),
            user_data_dir: PathBuf::from("/game/UserData"),
            java_path: "/opt/java/bin/java".to_string(),
            auth_args: vec!["--server".to_string(), "localhost:25565".to_string()],
            jvm_args: Some(vec!["-Xmx4G".to_string()]),
            env_vars: std::collections::HashMap::new(),
        };

        let cmd = build_game_command(ctx);
        let args: Vec<String> = cmd
            .as_std()
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect();

        // Expected argument sequence (snapshot)
        // Note: Order matters for game launch
        let expected_prefix = vec![
            "-Xmx4G",           // JVM arg
            "-jar",             // Java flag
            "/game/HytaleClient.jar",  // JAR path
            "--app-dir", "/game",
            "--user-dir", "/game/UserData",
            "--java-exec", "/opt/java/bin/java",
            "--uuid", "snapshot-uuid-001",
            "--name", "SnapshotTest",
            "--server", "localhost:25565",  // Auth args
        ];

        // Verify the prefix matches
        for (i, expected) in expected_prefix.iter().enumerate() {
            assert_eq!(
                &args[i], expected,
                "Argument {} mismatch: expected '{}', got '{}'",
                i, expected, args[i]
            );
        }
    }

    // === Linux-specific Tests ===

    #[cfg(target_os = "linux")]
    #[test]
    fn test_is_wayland_detection() {
        // This test just verifies the function doesn't panic
        // The actual result depends on the test environment
        let _ = is_wayland();
    }

    // === Edge Cases ===

    #[test]
    fn test_launch_context_empty_player_name() {
        let ctx = LaunchContext {
            player_name: "".to_string(),
            player_uuid: "uuid".to_string(),
            exec_path: PathBuf::from("/game/client.jar"),
            working_dir: PathBuf::from("/game"),
            user_data_dir: PathBuf::from("/userdata"),
            java_path: "java".to_string(),
            auth_args: vec![],
            env_vars: std::collections::HashMap::new(),
            jvm_args: None,
        };

        let cmd = build_game_command(ctx);
        let args: Vec<String> = cmd
            .as_std()
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect();

        // Empty name should still be passed
        assert!(args.contains(&"--name".to_string()));
    }

    #[test]
    fn test_launch_context_special_characters() {
        let ctx = LaunchContext {
            player_name: "Player with spaces & symbols!".to_string(),
            player_uuid: "uuid-with-dashes-123".to_string(),
            exec_path: PathBuf::from("/game/client.jar"),
            working_dir: PathBuf::from("/game/path with spaces"),
            user_data_dir: PathBuf::from("/userdata"),
            java_path: "java".to_string(),
            auth_args: vec!["--key=value with spaces".to_string()],
            env_vars: std::collections::HashMap::new(),
            jvm_args: None,
        };

        // Should not panic with special characters
        let cmd = build_game_command(ctx);
        let args: Vec<String> = cmd
            .as_std()
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect();

        assert!(args.contains(&"Player with spaces & symbols!".to_string()));
        assert!(args.contains(&"--key=value with spaces".to_string()));
    }
}
