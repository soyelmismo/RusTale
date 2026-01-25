use crate::config::OnlineFixMode;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub mod icons;
pub mod image_cache;

pub fn open_game_folder() {
    let path = crate::config::get_app_dir();
    open_path(path);
}

pub fn open_path(path: std::path::PathBuf) {
    if !path.exists() {
        let _ = std::fs::create_dir_all(&path);
    }

    let final_path = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => path,
    };

    println!("Opening folder: {:?}", final_path);

    if let Err(e) = open::that(final_path) {
        eprintln!("Error opening folder: {}", e);
    }
}

/// Sets execution permissions (755) on Unix systems.
/// Does nothing on other platforms.
pub async fn make_executable(path: &std::path::PathBuf) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if path.exists() {
            let meta = tokio::fs::metadata(path).await?;
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            tokio::fs::set_permissions(path, perms).await?;
        }
    }
    let _ = path;
    Ok(())
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
            return port;
        }
    }

    59313
}

pub fn get_saved_port() -> u16 {
    let server_root = crate::config::get_server_root_dir();
    let primary_path = server_root.join("server.port");

    let possible_paths = vec![
        primary_path,
        crate::config::get_app_dir().join("server.port"),
        std::path::PathBuf::from("server.port"),
    ];

    for p in possible_paths {
        if p.exists() {
            if let Ok(s) = std::fs::read_to_string(&p) {
                if let Ok(p_val) = s.trim().parse::<u16>() {
                    return p_val;
                }
            }
        }
    }

    59313
}

pub fn run_java_proxy_logic(online_mode: OnlineFixMode) -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    proxy_log("--- PROXY STARTED ---");
    proxy_log(&format!("Raw Args: {:?}", args));

    let current_exe = std::env::current_exe()?;
    let bin_dir = current_exe.parent().unwrap();

    let java_original_name = if cfg!(windows) {
        "java_original.exe"
    } else {
        "java_original"
    };
    let java_default_name = if cfg!(windows) { "java.exe" } else { "java" };

    let mut java_real = bin_dir.join(java_original_name);

    if !java_real.exists() {
        proxy_log("[WARN] java_original not found, checking side-by-side java...");
        java_real = bin_dir.join(java_default_name);

        if java_real == current_exe {
            proxy_log(
                "[CRITICAL] Recursive loop detected! We are java.exe but java_original is missing.",
            );
            return Err(anyhow::anyhow!(
                "Recursive Proxy Loop: java_original missing"
            ));
        }
    } else {
        proxy_log(&format!(
            "[INFO] Hijack mode active. Real java: {:?}",
            java_real
        ));
    }

    let mut final_args = args.clone();

    proxy_log(&format!("CWD: {:?}", std::env::current_dir()));
    let cwd_res = std::env::current_dir();

    let port = get_saved_port();
    let mode_str = match online_mode {
        OnlineFixMode::Sanasol => "sanasol",
        OnlineFixMode::Local => "local",
    };

    // 2. Scan for server.jar
    proxy_log("Scanning arguments for HytaleServer.jar...");
    for (i, arg) in args.iter().enumerate() {
        if arg.to_lowercase().ends_with("hytaleserver.jar") {
            proxy_log(&format!("Found candidate arg: {}", arg));

            let mut original_jar_path = std::path::PathBuf::from(arg);

            // Try to resolve absolute path if it doesn't exist
            if !original_jar_path.exists() {
                if let Ok(cwd) = &cwd_res {
                    let abs = cwd.join(arg);
                    if abs.exists() {
                        original_jar_path = abs;
                        proxy_log(&format!(
                            "Resolved relative path to: {:?}",
                            original_jar_path
                        ));
                    }
                }
            }

            if original_jar_path.exists() {
                proxy_log(&format!(
                    "Intercepting Server JAR at: {:?}",
                    original_jar_path
                ));

                let server_dir = original_jar_path
                    .parent()
                    .unwrap_or(std::path::Path::new("."));

                let possible_original = server_dir.join("HytaleServer.original");
                if possible_original.exists() {
                    proxy_log(
                        "Detected persistent swap (HytaleServer.original exists). Assuming input arg is patched.",
                    );
                    break;
                }

                let patched_jar_name = format!("HytaleServer.{}.{}.jar", mode_str, port);
                let patched_jar_path = server_dir.join(patched_jar_name);

                if patched_jar_path.exists() {
                    proxy_log("Persistent patched JAR found. Using it.");
                    final_args[i] = patched_jar_path.to_string_lossy().to_string();
                } else {
                    proxy_log("Patched JAR not found. Patching on-the-fly...");
                    if let Ok(_) = crate::game::patcher::patch_server_jar(
                        &original_jar_path,
                        &patched_jar_path,
                        online_mode,
                        port,
                    ) {
                        final_args[i] = patched_jar_path.to_string_lossy().to_string();
                    }
                }
                break;
            } else {
                proxy_log(&format!("File not found: {:?}", original_jar_path));
            }
        }
    }

    // Launch the real Java
    use std::process::Command;
    proxy_log("Launching real java...");

    #[cfg(target_os = "windows")]
    use std::os::windows::process::CommandExt;

    let mut cmd = Command::new(java_real);
    cmd.args(final_args);

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000);
    }

    let status = cmd.status()?;

    if let Some(code) = status.code() {
        std::process::exit(code);
    }
    Ok(())
}
// Simple helper for proxy debug logs (since stdout can be lost)
fn proxy_log(msg: &str) {
    use std::io::Write;
    let path = std::env::current_dir().unwrap().join("RusTale_Proxy.log");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(
            file,
            "[{}] {}",
            chrono::Local::now().format("%H:%M:%S"),
            msg
        );
    }
    // Also print to stdout for safety
    println!("{}", msg);
}

pub async fn dir_size(path: impl AsRef<Path>) -> Result<u64> {
    let mut total_size = 0;
    let mut entries = tokio::fs::read_dir(path).await?;
    while let Some(entry) = entries.next_entry().await? {
        let meta = entry.metadata().await?;
        if meta.is_dir() {
            total_size += Box::pin(dir_size(entry.path())).await?;
        } else {
            total_size += meta.len();
        }
    }
    Ok(total_size)
}

// Function for moving directory with progress reporting
pub async fn move_dir_with_progress<F>(src: PathBuf, dst: PathBuf, on_progress: F) -> Result<()>
where
    F: Fn(f32) + Send + Sync + 'static + Clone,
{
    if !src.exists() {
        return Ok(());
    }
    if src == dst {
        return Ok(());
    }

    // 1. Detect if we are running inside the source folder
    let current_exe = std::env::current_exe().unwrap_or_default();
    let is_self_contained = current_exe.starts_with(&src);

    // 2. Calculate total size
    let total_bytes = dir_size(&src).await?;
    let copied_bytes = Arc::new(AtomicU64::new(0));

    // 3. Recursive copy internal
    async fn copy_recursive<F>(
        src: PathBuf,
        dst: PathBuf,
        total: u64,
        current: Arc<AtomicU64>,
        cb: F,
    ) -> Result<()>
    where
        F: Fn(f32) + Send + Sync + 'static + Clone,
    {
        tokio::fs::create_dir_all(&dst).await?;
        let mut entries = tokio::fs::read_dir(&src).await?;

        while let Some(entry) = entries.next_entry().await? {
            let ty = entry.file_type().await?;
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());

            if ty.is_dir() {
                Box::pin(copy_recursive(
                    src_path,
                    dst_path,
                    total,
                    current.clone(),
                    cb.clone(),
                ))
                .await?;
            } else {
                tokio::fs::copy(&src_path, &dst_path).await?;
                let len = entry.metadata().await?.len();
                let prev = current.fetch_add(len, Ordering::Relaxed);

                // Report progress
                if total > 0 {
                    let pct = ((prev + len) as f64 / total as f64 * 100.0) as f32;
                    cb(pct);
                }
            }
        }
        Ok(())
    }

    // Execute copy
    copy_recursive(src.clone(), dst, total_bytes, copied_bytes, on_progress).await?;

    // 4. Intelligent Cleanup
    if is_self_contained {
        println!(
            "[Migration] Running executable is inside source dir. Performing selective cleanup."
        );
        // Delete everything EXCEPT the current executable
        if let Err(e) = remove_dir_recursive_exclude(&src, &current_exe).await {
            eprintln!("[Migration] Warning during cleanup: {}", e);
            // We don't return fatal error here, because the copy (the important data) is already done.
        }
    } else {
        // Standard full deletion
        tokio::fs::remove_dir_all(&src)
            .await
            .context("Failed to remove old directory")?;
    }

    Ok(())
}

/// Recursively deletes but skips a specific file (the executable)
async fn remove_dir_recursive_exclude(dir: &Path, exclude_file: &Path) -> Result<()> {
    let mut entries = tokio::fs::read_dir(dir).await?;
    let mut is_empty = true;

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();

        // If it's the file we want to protect, skip it
        if path == exclude_file {
            is_empty = false;
            continue;
        }

        if entry.file_type().await?.is_dir() {
            // Recursion
            if let Err(_) = Box::pin(remove_dir_recursive_exclude(&path, exclude_file)).await {
                is_empty = false;
            }
            // Try to delete folder if it became empty
            if tokio::fs::remove_dir(&path).await.is_err() {
                is_empty = false; // Could not delete, probably contains the exe inside
            }
        } else {
            // It's a normal file, delete it
            let _ = tokio::fs::remove_file(&path).await;
        }
    }

    // Try to delete root directory (only works if empty, i.e., didn't contain the exe)
    if is_empty {
        let _ = tokio::fs::remove_dir(dir).await;
    }

    Ok(())
}
