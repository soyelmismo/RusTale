use crate::config::OnlineFixMode;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(target_os = "linux")]
use libc;

pub mod icons;
pub mod image_cache;
pub mod win_job;

/// Validates that the Java executable path is within expected bounds
fn validate_java_executable(java_path: &PathBuf, bin_dir: &Path) -> Result<()> {
    // Ensure the Java executable is within the same directory as the launcher
    if let Some(java_parent) = java_path.parent() {
        // FIX: same_as no existe. Usamos canonicalize para comparar rutas reales
        // Usamos unwrap_or para no fallar si el archivo aun no existe (raro pero posible),
        // en cuyo caso fallback a la ruta original
        let canon_java_dir = java_parent
            .canonicalize()
            .unwrap_or(java_parent.to_path_buf());
        let canon_bin_dir = bin_dir.canonicalize().unwrap_or(bin_dir.to_path_buf());

        if canon_java_dir != canon_bin_dir {
            return Err(anyhow::anyhow!(
                "Security violation: Java executable outside expected directory: {:?} vs {:?}",
                java_path,
                bin_dir
            ));
        }
    }

    // Additional check: ensure we're not executing something unexpected
    let java_name = java_path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    // Convert to lowercase to be safe on Windows
    if !java_name.to_lowercase().contains("java") {
        return Err(anyhow::anyhow!(
            "Security violation: Attempted to execute non-Java binary: {}",
            java_name
        ));
    }

    Ok(())
}

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
    // 1. PRIMERO: Intentar revivir el puerto guardado (Recuperacion de sesion)
    let saved_port = get_saved_port();

    // Verificamos si podemos bindearlo nosotros (esta libre)
    if std::net::TcpListener::bind(("127.0.0.1", saved_port)).is_ok() {
        println!("[Port] Successfully recovered saved port {}", saved_port);
        // Devolvemos el puerto guardado para mantener el Issuer URL estatico
        return saved_port;
    }

    // 2. Si no pudimos bindearlo, es porque OTRO proceso (¿Server vivo?) lo tiene
    // En ese caso, este codigo no deberia ejecutarse para levantar un servidor,
    // sino para conectar. Pero si estamos aqui para buscar puerto para servidor nuevo...
    // Generamos uno nuevo.

    use rand::Rng;
    let mut rng = rand::rng();

    for _ in 0..100 {
        let port = rng.random_range(10000..=65535);
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            // ¡IMPORTANTE! Solo guardar si es nuevo.
            save_active_port(port);
            return port;
        }
    }

    59313
}

pub fn get_runtime_port_file() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        // Resuelve /run/user/1000/rustale/auth.port automaticamente
        // Prioridad 1: XDG_RUNTIME_DIR (estandar Linux)
        if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
            let path = PathBuf::from(runtime_dir).join("rustale");
            let _ = std::fs::create_dir_all(&path);
            return path.join("auth.port");
        }

        // Prioridad 2: Construccion manual usando UID (fallback robusto)
        let uid = unsafe { libc::getuid() };
        let fallback_path = PathBuf::from(format!("/run/user/{}/rustale", uid));
        let _ = std::fs::create_dir_all(&fallback_path);
        return fallback_path.join("auth.port");
    }
    // En Windows seguimos usando la carpeta del servidor para consistencia
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
    println!("[SEAMLESS] No active port found, using default 59313");
    59313 // Default si no hay nadie corriendo
}

#[cfg(target_os = "windows")]
pub unsafe fn get_parent_pid() -> u32 {
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32, Process32First, Process32Next, TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;

    unsafe {
        let pid = GetCurrentProcessId();
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            return 0;
        }

        let mut entry: PROCESSENTRY32 = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32>() as u32;

        if Process32First(snapshot, &mut entry) != 0 {
            loop {
                if entry.th32ProcessID == pid {
                    windows_sys::Win32::Foundation::CloseHandle(snapshot);
                    return entry.th32ParentProcessID;
                }
                if Process32Next(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        windows_sys::Win32::Foundation::CloseHandle(snapshot);
        0
    }
}

pub fn run_java_proxy_logic(online_mode: OnlineFixMode) -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    println!("--- PROXY STARTED ---");
    println!("Raw Args: {:?}", args);

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
        println!("[WARN] java_original not found, checking side-by-side java...");
        java_real = bin_dir.join(java_default_name);

        if java_real == current_exe {
            println!(
                "[CRITICAL] Recursive loop detected! We are java.exe but java_original is missing.",
            );
            return Err(anyhow::anyhow!(
                "Recursive Proxy Loop: java_original missing"
            ));
        }
    } else {
        println!("[INFO] Hijack mode active. Real java: {:?}", java_real);
    }

    // Validate Java executable path for security
    if let Err(e) = validate_java_executable(&java_real, bin_dir) {
        println!("[SECURITY] {}", e);
        return Err(e);
    }

    println!("CWD: {:?}", std::env::current_dir());
    let cwd_res = std::env::current_dir();

    let port = get_saved_port();
    let mode_str = match online_mode {
        OnlineFixMode::Sanasol => "sanasol",
        OnlineFixMode::Local => "local",
    };

    // Launch the real Java
    #[cfg(target_os = "windows")]
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    let mut cmd = Command::new(java_real);

    // Inject DualAuth environment variables for the new patcher
    if online_mode == OnlineFixMode::Local {
        cmd.env("HYTALE_AUTH_DOMAIN", format!("127.0.0.000001:{}", port));
    } else {
        cmd.env("HYTALE_AUTH_DOMAIN", "sessions.sanasol.ws");
    }

    // Disable Sentry for proxy
    cmd.env("DISABLE_SENTRY", "1");

    // --- NEW: JAVA AGENT INJECTION (SMART CHECK) ---
    // The agent is located at RusTale/tools/dualauth-agent.jar
    // We are at RusTale/tools/jre/latest/bin/java.exe (Proxy)
    // Hierarchy: tools -> jre -> latest -> bin -> java.exe
    if let Some(latest_dir) = bin_dir.parent() {      // tools/jre/latest
        if let Some(jre_dir) = latest_dir.parent() {  // tools/jre
            if let Some(tools_dir) = jre_dir.parent() { // tools
                let agent_path = tools_dir.join("dualauth-agent.jar");
                if agent_path.exists() {
                    // FIX CRÍTICO: Verificar si el argumento YA ESTÁ PRESENTE.
                    // Esto evita la duplicación cuando server/runner.rs ya lo ha añadido.
                    let agent_filename = agent_path.file_name().and_then(|f| f.to_str()).unwrap_or("dualauth-agent.jar");
                    let already_present = args.iter().any(|a| a.contains("-javaagent") && a.contains(agent_filename));

                    if !already_present {
                        println!("[Proxy] Injecting Java Agent: {:?}", agent_path);
                        // IMPORTANTE: Usamos un método que pone el agente AL PRINCIPIO de los args internos
                        // Sin embargo, Command no tiene 'prepend'.
                        // Dado que args se añade despues, cmd.arg aqui añade ANTES de los argumentos del juego.
                        // Correcto orden: java [proxy_args] [runner_args]
                        cmd.arg(format!("-javaagent:{}", agent_path.to_string_lossy()));
                    } else {
                        println!("[Proxy] Java Agent already present in arguments. Skipping proxy injection.");
                    }
                } else {
                    println!("[Proxy] WARNING: Java Agent NOT FOUND at {:?}", agent_path);
                }
            }
        }
    }

    // Launch the real Java with telemetry disabled by default
    cmd.args(&args).arg("--disable-sentry");
    println!("Launching real java...");

    #[cfg(target_os = "windows")]
    {
        // Forzamos la NO creacion de ventana independientemente de si es servidor.
        // Esto se debe a que el Proxy ya se encarga de heredar los canales de logs.
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    // Explicitly inherit stdio to ensure logs pass through the proxy
    cmd.stdout(std::process::Stdio::inherit());
    cmd.stderr(std::process::Stdio::inherit());
    cmd.stdin(std::process::Stdio::inherit());

    #[cfg(target_os = "windows")]
    {
        let mut child = cmd.spawn()?;

        // Use JobObject to ensure child survives or dies with the proxy
        let job = if let Ok(job) = win_job::JobObject::new() {
            use std::os::windows::io::AsRawHandle;
            let handle = child.as_raw_handle() as _;
            if let Err(e) = job.add_process(handle) {
                println!("[WARN] Failed to assign child to JobObject: {}", e);
                None
            } else {
                println!("[INFO] Child assigned to JobObject (auto-kill enabled)");
                Some(job)
            }
        } else {
            println!("[ERROR] Failed to create JobObject. Child might become orphan.");
            None
        };

        // --- PARENT WATCHDOG START ---
        unsafe {
            use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
            use windows_sys::Win32::System::Threading::{
                INFINITE, OpenProcess, WaitForMultipleObjects,
            };
            const SYNCHRONIZE: u32 = 1048576; // 0x00100000
            use std::os::windows::io::AsRawHandle;

            let parent_pid = get_parent_pid();
            if parent_pid != 0 {
                let parent_handle = OpenProcess(SYNCHRONIZE, 0, parent_pid);
                if !parent_handle.is_null() {
                    let child_handle =
                        child.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE;
                    let handles = [child_handle, parent_handle];

                    // Wait for either the child to exit (normal) or parent to exit (orphan scenario)
                    let result = WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE);

                    if result == WAIT_OBJECT_0 + 1 {
                        // Parent died (Index 1)
                        println!("[INFO] Parent process died. Killing child...");
                        let _ = child.kill();
                    }

                    windows_sys::Win32::Foundation::CloseHandle(parent_handle);
                }
            }
        }
        // --- PARENT WATCHDOG END ---

        let status = child.wait()?;

        // Explicitly drop job after wait
        drop(job);

        if let Some(code) = status.code() {
            std::process::exit(code);
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let status = cmd.status()?;
        if let Some(code) = status.code() {
            std::process::exit(code);
        }
    }

    Ok(())
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

/// Simple recursive copy WITHOUT callback/progress complexity
/// Useful for quick internal copies
pub fn copy_recursive_sync(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> Result<()> {
    std::fs::create_dir_all(&dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dst_path = dst.as_ref().join(entry.file_name());
        if ty.is_dir() {
            copy_recursive_sync(entry.path(), dst_path)?;
        } else {
            std::fs::copy(entry.path(), dst_path)?;
        }
    }
    Ok(())
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

/// Helper para limpiar rutas, especialmente en Windows (eliminar \\?\)
pub fn sanitize_path(path: &std::path::PathBuf) -> std::path::PathBuf {
    // 1. Obtener ruta absoluta canónica
    let absolute = path.canonicalize().unwrap_or(path.clone());
    
    // 2. Si estamos en Windows, quitar el prefijo UNC extendido
    #[cfg(windows)]
    {
        let str_path = absolute.to_string_lossy().to_string();
        if str_path.starts_with(r"\\?\") {
            return std::path::PathBuf::from(&str_path[4..]);
        }
    }
    
    absolute
}
