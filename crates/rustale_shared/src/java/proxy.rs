use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::io::{Read, Write};
use crate::config::OnlineFixMode;
use crate::paths::GamePaths;
use super::tracking::track_process;

#[cfg(windows)]
use crate::java::win_job;

pub fn setup_java_proxy(java_real: &PathBuf) -> Result<PathBuf> {
    let bin_dir = java_real
        .parent()
        .ok_or_else(|| anyhow::anyhow!("No parent dir"))?;
    let exe_name = if cfg!(windows) { "java.exe" } else { "java" };
    let original_name = if cfg!(windows) {
        "java_original.exe"
    } else {
        "java_original"
    };

    let java_proxy = bin_dir.join(exe_name);
    let java_original = bin_dir.join(original_name);

    if !java_original.exists() {
        std::fs::rename(java_real, &java_original)?;
    }

    // Always overwrite the proxy binary to ensure it matches the current launcher version
    let current_exe = std::env::current_exe()?;
    if let Err(e) = std::fs::copy(&current_exe, &java_proxy) {
        // If we can't copy (e.g. file busy), and it exists, we might warn but proceed.
        // However, for development/updates, this is critical.
        eprintln!(
            "[JavaProxy] Warning: Failed to update java proxy binary: {}",
            e
        );
        if !java_proxy.exists() {
            return Err(e.into());
        }
    }

    Ok(java_proxy)
}

pub fn remove_java_proxy(java_real: &PathBuf) -> Result<()> {
    let bin_dir = java_real
        .parent()
        .ok_or_else(|| anyhow::anyhow!("No parent dir"))?;
    let exe_name = if cfg!(windows) { "java.exe" } else { "java" };
    let original_name = if cfg!(windows) {
        "java_original.exe"
    } else {
        "java_original"
    };

    let java_proxy = bin_dir.join(exe_name);
    let java_original = bin_dir.join(original_name);

    if java_original.exists() {
        if java_proxy.exists() {
            let _ = std::fs::remove_file(&java_proxy);
        }
        std::fs::rename(&java_original, &java_proxy)?;
    }
    Ok(())
}

pub fn get_runtime_port_file() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        // Resuelve /run/user/1000/rustale/auth.port automaticamente
        if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
            let path = PathBuf::from(runtime_dir).join("rustale");
            let _ = std::fs::create_dir_all(&path);
            return path.join("auth.port");
        }

        let uid = unsafe { libc::getuid() };
        let fallback_path = PathBuf::from(format!("/run/user/{}/rustale", uid));
        let _ = std::fs::create_dir_all(&fallback_path);
        return fallback_path.join("auth.port");
    }
    #[cfg(target_os = "windows")]
    crate::config::get_server_root_dir().join("server.port")
}

pub fn save_active_port(port: u16) {
    let path = get_runtime_port_file();
    if let Err(e) = std::fs::write(&path, port.to_string()) {
        eprintln!("[WARNING] Failed to save port to {:?}: {}", path, e);
    } else {
        println!("[SEAMLESS] Port {} saved to runtime storage", port);
    }
}

pub fn get_saved_port() -> u16 {
    let path = get_runtime_port_file();
    if let Ok(s) = std::fs::read_to_string(&path) {
        if let Ok(p_val) = s.trim().parse::<u16>() {
            println!(
                "[SEAMLESS] Found active port {} from runtime storage",
                p_val
            );
            return p_val;
        }
    }
    59313 // Default
}

/// Detects if current process is running as a Java Proxy
pub fn is_running_as_java_proxy() -> bool {
    // 0. Safe fallback: Environment variable
    if std::env::var("RUSTALE_IS_PROXY").is_ok() {
        return true;
    }
    // Checking for AURORA_MODE is a strong signal we are the proxy
    if std::env::var("AURORA_MODE").is_ok() {
        return true;
    }

    // Check arguments typical of Java invocations
    for arg in std::env::args().skip(1) {
        if arg.starts_with("-X") || arg.starts_with("-D") || arg == "-jar" || arg == "-cp" {
            return true;
        }
    }

    // Heuristic Check: Are we named "java" or sitting next to "java_original"?
    if let Ok(exe) = std::env::current_exe() {
        if let Some(name) = exe.file_stem() {
            let name_str = name.to_string_lossy().to_lowercase();
            if name_str.contains("rustale_proxy") {
                return true;
            }
            if name_str == "java" {
                if let Some(dir) = exe.parent() {
                    let original_name = if cfg!(windows) {
                        "java_original.exe"
                    } else {
                        "java_original"
                    };

                    if dir.join(original_name).exists() {
                        return true;
                    }
                }
            }
        }
    }
    false
}

pub fn find_free_port() -> u16 {
    let saved_port = get_saved_port();
    if std::net::TcpListener::bind(("127.0.0.1", saved_port)).is_ok() {
        return saved_port;
    }

    use rand::Rng;
    let mut rng = rand::rng();

    for _ in 0..100 {
        let port = rng.random_range(10000..=65535);
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            save_active_port(port);
            return port;
        }
    }
    59313
}

/// Validates that the Java executable path is within expected bounds
fn validate_java_executable(java_path: &PathBuf, bin_dir: &Path) -> Result<()> {
    if let Some(java_parent) = java_path.parent() {
        let canon_java_dir = java_parent
            .canonicalize()
            .unwrap_or(java_parent.to_path_buf());
        let canon_bin_dir = bin_dir.canonicalize().unwrap_or(bin_dir.to_path_buf());

        if canon_java_dir != canon_bin_dir {
            return Err(anyhow::anyhow!(
                "Security violation: Java executable outside expected directory"
            ));
        }
    }

    let java_name = java_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if !java_name.to_lowercase().contains("java") {
        return Err(anyhow::anyhow!("Security violation: Non-Java binary"));
    }
    Ok(())
}

pub fn run_java_proxy_logic(online_mode: OnlineFixMode) -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let current_exe = std::env::current_exe()?;
    let bin_dir = current_exe.parent().context("No parent dir")?;

    let java_original_name = if cfg!(windows) { "java_original.exe" } else { "java_original" };
    let java_default_name = if cfg!(windows) { "java.exe" } else { "java" };

    let mut java_real = bin_dir.join(java_original_name);
    if !java_real.exists() {
        java_real = bin_dir.join(java_default_name);
        if java_real == current_exe {
            return Err(anyhow::anyhow!("Recursive Proxy Loop"));
        }
    }

    validate_java_executable(&java_real, bin_dir)?;

    let port = get_saved_port();
    let mut cmd = std::process::Command::new(java_real);

    if online_mode == OnlineFixMode::Local {
        cmd.env("HYTALE_AUTH_DOMAIN", format!("127.0.0.000001:{}", port));
    } else {
        cmd.env("HYTALE_AUTH_DOMAIN", "sessions.sanasol.ws");
    }

    cmd.env("DISABLE_SENTRY", "1");

    let agent_path = if std::env::var("RUSTALE_IS_SERVER").is_ok() {
        GamePaths::new(crate::config::get_server_root_dir()).dualauth_agent()
    } else {
        GamePaths::new(crate::config::get_app_dir()).dualauth_agent()
    };

    if agent_path.exists() {
        let agent_arg = format!("-javaagent:{}", agent_path.to_string_lossy());
        if !args.iter().any(|a| a == &agent_arg) {
            cmd.arg(agent_arg);
        }
    }

    cmd.args(&args).arg("--disable-sentry");
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::inherit());
    cmd.stderr(std::process::Stdio::inherit());

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                libc::setpgid(0, 0);
                Ok(())
            });
        }
    }

    let mut child = cmd.spawn().context("Failed to spawn java process")?;
    
    // Trackear el PID del proceso Java creado
    let pid = child.id();
    track_process(pid, "java_proxy".to_string());
    
    let child_stdin = child.stdin.take().context("Failed to open child stdin")?;
    let shared_stdin = Arc::new(std::sync::Mutex::new(child_stdin));

    let stdin_writer_user = shared_stdin.clone();
    let stdin_writer_signal = shared_stdin.clone();

    std::thread::spawn(move || {
        let mut buffer = [0u8; 1024];
        let mut console_in = std::io::stdin();
        loop {
            match console_in.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    if let Ok(mut writer) = stdin_writer_user.lock() {
                        if writer.write_all(&buffer[..n]).is_err() || writer.flush().is_err() {
                            break;
                        }
                    }
                }
                Err(_) => break,
            }
        }
    });

    let stop_sent = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_sent_c = stop_sent.clone();

    let _ = ctrlc::set_handler(move || {
        if stop_sent_c.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        stop_sent_c.store(true, std::sync::atomic::Ordering::Relaxed);

        if let Ok(mut writer) = stdin_writer_signal.lock() {
            let _ = writer.write_all(b"shutdown\n");
            let _ = writer.flush();
        }
    });

    #[cfg(target_os = "windows")]
    let _job = if let Ok(job) = win_job::JobObject::new() {
        use std::os::windows::io::AsRawHandle;
        let _ = job.add_process(child.as_raw_handle() as _);
        Some(job)
    } else {
        None
    };

    let status = child.wait()?;
    if let Some(code) = status.code() {
        std::process::exit(code);
    }

    Ok(())
}
