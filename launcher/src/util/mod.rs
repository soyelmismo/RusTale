use crate::config::OnlineFixMode;
use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::io::{Read, Write};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use libc;

pub mod icons;
pub mod image_cache;
pub mod win_job;

/// Cache global para evitar syscalls repetidas de current_exe()
static CURRENT_EXE: Lazy<anyhow::Result<PathBuf>> = Lazy::new(|| {
    std::env::current_exe().context("Failed to get current executable path")
});

/// Última actividad registrada (para giro predictivo)
static LAST_ACTIVITY: Lazy<std::sync::Mutex<Instant>> = Lazy::new(|| {
    std::sync::Mutex::new(Instant::now())
});

/// Contador de giros automáticos realizados
static AUTO_TRIM_COUNT: AtomicU64 = AtomicU64::new(0);

/// Contador de giros consecutivos sin liberación significativa
static CONSECUTIVE_WEAK_TRIMS: AtomicU64 = AtomicU64::new(0);

/// Umbral de memoria para considerar que un giro fue débil (en MB)
const WEAK_TRIM_THRESHOLD_MB: f64 = 10.0;

/// Obtiene la ruta del ejecutable actual (cacheada)
pub fn get_current_exe() -> anyhow::Result<&'static PathBuf> {
    CURRENT_EXE.as_ref().map_err(|e| anyhow::anyhow!("{}", e))
}

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

pub fn run_java_proxy_logic(online_mode: OnlineFixMode) -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    println!("--- PROXY STARTED ---");
    println!("Raw Args: {:?}", args);

    let current_exe = get_current_exe()?;
    let bin_dir = current_exe.parent().context("Failed to get executable directory")?;

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

        if java_real == *current_exe {
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
    let _cwd_res = std::env::current_dir();

    let port = get_saved_port();
    let _mode_str = match online_mode {
        OnlineFixMode::Sanasol => "sanasol",
        OnlineFixMode::Local => "local",
    };

    // Launch the real Java
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
    // Use GamePaths with the proper data_dir
    // Always use the main app directory's tools for the agent
    let agent_path = {
        let paths = crate::game::paths::GamePaths::new(crate::config::get_app_dir());
        paths.dualauth_agent()
    };
    
    if agent_path.exists() {
        // Check if Java Agent is already present to avoid duplication
        let agent_arg = format!("-javaagent:{}", agent_path.to_string_lossy());
        let already_present = args.iter().any(|a| a == &agent_arg);

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

    // Launch the real Java with telemetry disabled by default
    cmd.args(&args).arg("--disable-sentry");
    println!("Launching real java...");

    // [BLOQUE FINAL PARA SERVER RUNNER]
    
    // 1. Configuramos PIPED solo para stdin (necesitamos inyectar comandos).
    // stdout/stderr se heredan para que veas el log, pero Java será tolerante si la ventana se cierra.
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::inherit());
    cmd.stderr(std::process::Stdio::inherit());

    // === WINDOWS ISOLATION ===
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }

    // === LINUX/MAC ISOLATION ===
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::process::CommandExt;
        // setpgid(0, 0) crea un nuevo Grupo de Procesos donde el líder es el hijo.
        // Esto aisla al hijo de las señales broadcast (SIGHUP/SIGINT) que recibe el padre.
        unsafe {
            cmd.pre_exec(|| {
                // Importante: Esto requiere que tu Cargo.toml tenga "libc"
                libc::setpgid(0, 0); 
                Ok(())
            });
        }
    }

    // Ahora lanzamos. 
    // En Linux: Java estará en su propio PGID y no morirá al cerrar la terminal.
    // Rust recibirá la señal, escribirá "shutdown", Java obedecerá.
    let mut child = cmd.spawn().context("Failed to spawn server process")?;

    // Capturamos el stdin de Java para uso exclusivo de Rust
    let child_stdin = child.stdin.take().context("Failed to open child stdin")?;
    
    // Compartimos el stdin entre el usuario (teclado) y el sistema de emergencia
    let shared_stdin = Arc::new(std::sync::Mutex::new(child_stdin));
    
    let stdin_writer_user = shared_stdin.clone();
    let stdin_writer_signal = shared_stdin.clone();

    // HILO 1: Pasarela de Comandos (Usuario escribe -> Java recibe)
    std::thread::spawn(move || {
        let mut buffer = [0u8; 1024];
        let mut console_in = std::io::stdin();
        loop {
            // Si la ventana se cierra, 'read' devolverá error o 0 inmediatamente.
            // Manejamos eso silenciosamente para que el hilo simplemente muera sin pánico.
            match console_in.read(&mut buffer) {
                Ok(0) => break, // EOF (Consola cerrada o Ctrl+Z)
                Ok(n) => {
                    if let Ok(mut writer) = stdin_writer_user.lock() {
                        if writer.write_all(&buffer[..n]).is_err() { break; }
                        if writer.flush().is_err() { break; }
                    }
                }
                Err(_) => break, // Error de lectura (ej: ventana destruida)
            }
        }
    });

    // HILO 2: Guardián de Cierre (Detecta X, Alt+F4, Ctrl+C)
    // Usamos una variable atómica para asegurar que solo enviamos el comando una vez.
    let stop_sent = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_sent_c = stop_sent.clone();

    // CONFIGURAMOS EL TRAPO DE SEÑALES (Funciona para Click en X en Windows también)
    let _ = ctrlc::set_handler(move || {
        // Evitamos enviar el comando múltiples veces si el usuario spamea Ctrl+C
        if stop_sent_c.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        stop_sent_c.store(true, std::sync::atomic::Ordering::Relaxed);

        // Intentamos imprimir alerta. Si la ventana ya no existe (click en X), 
        // println! podría fallar, así que ignoramos errores.
        let _ = std::panic::catch_unwind(|| {
            println!("\n[RusTale] CLOSING EVENT DETECTED!");
            println!("[RusTale] Injecting 'shutdown' command to safeguard world data...");
        });

        // INYECCIÓN QUIRÚRGICA: Escribimos "shutdown" directo a la memoria de Java
        if let Ok(mut writer) = stdin_writer_signal.lock() {
            // Escribimos "shutdown" + salto de linea explícito
            // Ignoramos resultado por si Java ya murió antes que nosotros.
            let _ = writer.write_all(b"shutdown\n");
            let _ = writer.flush();
        }
        
        // NO hacemos exit(). Dejamos que este closure termine. 
        // El hilo principal (abajo) está esperando a Java (child.wait()).
        // Al enviar 'shutdown', Java empezará a guardar y cerrarse solo.
    });

    // [WINDOWS SAFETY] Vinculamos el proceso al JobObject.
    // Esto asegura que si Rust crashea fuerte o es asesinado por el Admin de Tareas, Java muere también.
    // Evita servidores zombies consumiendo RAM en segundo plano.
    #[cfg(target_os = "windows")]
    let _job = if let Ok(job) = win_job::JobObject::new() {
        use std::os::windows::io::AsRawHandle;
        let _ = job.add_process(child.as_raw_handle() as _);
        Some(job)
    } else {
        None
    };

    // --- BLOQUEO PRINCIPAL ---
    // Rust se quedará congelado aquí hasta que Java termine.
    // Como inyectamos "shutdown" arriba, Java terminará voluntariamente pronto.
    let status = child.wait()?;

    if let Some(code) = status.code() {
        std::process::exit(code);
    }

    Ok(())
    // [MODIFICACIÓN FIN]
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
    let is_self_contained = match get_current_exe() {
        Ok(exe) => exe.starts_with(&src),
        Err(_) => false,
    };

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
        if let Ok(exe) = get_current_exe() {
            if let Err(e) = remove_dir_recursive_exclude(&src, exe).await {
                eprintln!("[Migration] Warning during cleanup: {}", e);
                // We don't return fatal error here, because the copy (the important data) is already done.
            }
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

/// Obtiene el uso actual de memoria RSS (Resident Set Size) en bytes
/// Returns 0 si falla la medición
fn get_memory_usage() -> u64 {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::System::ProcessStatus::PROCESS_MEMORY_COUNTERS;
        use windows_sys::Win32::System::ProcessStatus::K32GetProcessMemoryInfo;
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        use std::mem;
        
        unsafe {
            let mut pmc: PROCESS_MEMORY_COUNTERS = mem::zeroed();
            pmc.cb = mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            
            if K32GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc, pmc.cb) != 0 {
                pmc.WorkingSetSize as u64
            } else {
                0
            }
        }
    }
    
    #[cfg(target_os = "linux")]
    {
        use std::fs;
        
        // Leer /proc/self/stat para obtener RSS
        if let Ok(stat) = fs::read_to_string("/proc/self/stat") {
            // El campo 23 (índice 22) es RSS en páginas
            if let Some(fields) = stat.split_whitespace().collect::<Vec<_>>().get(23) {
                if let Ok(pages) = fields.parse::<u64>() {
                    // Convertir páginas a bytes (tamaño de página usualmente 4096)
                    return pages * 4096;
                }
            }
        }
        0
    }
    
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        0 // No implementado para otras plataformas
    }
}

/// Registra actividad para el sistema de giro predictivo
pub fn register_activity() {
    if let Ok(mut last) = LAST_ACTIVITY.lock() {
        *last = Instant::now();
    }
}

/// Verifica si ha pasado suficiente tiempo de inactividad para un giro automático
pub fn check_auto_trim() {
    const INACTIVITY_THRESHOLD: Duration = Duration::from_secs(30); // 30 segundos de inactividad
    
    if let Ok(mut last) = LAST_ACTIVITY.lock() {
        if last.elapsed() > INACTIVITY_THRESHOLD {
            // Realizar giro predictivo
            trim_memory_predictive();
            *last = Instant::now(); // Resetear timer
            
            // Incrementar contador
            let count = AUTO_TRIM_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
            println!("[GIRO] 🤖 Auto-giro #{} por inactividad ({}s)", count, INACTIVITY_THRESHOLD.as_secs());
        }
    }
}

/// Versión de trim_memory() para giros predictivos con auto-escalado
fn trim_memory_predictive() {
    // Determinar nivel basado en giros consecutivos débiles
    let consecutive_weak = CONSECUTIVE_WEAK_TRIMS.load(Ordering::Relaxed);
    
    let level = match consecutive_weak {
        0..=2 => TrimLevel::Normal,      // Primeros 3 giros: Normal
        3..=5 => TrimLevel::Aggressive,   // Giros 4-6: Agresivo
        _ => TrimLevel::Extreme,            // Más de 6: Extremo
    };
    
    println!("[AUTO-ESCALA] Nivel {:?} ({} giros débiles consecutivos)", level, consecutive_weak);
    trim_memory_with_level(level);
}

/// Niveles de agresividad para el giro de memoria
#[derive(Debug, Clone, Copy)]
pub enum TrimLevel {
    /// Normal: recolección estándar con medición
    Normal,
    /// Agresivo: limpieza completa con múltiples pasadas
    Aggressive,
    /// Extremo: modo servidor - todo lo posible
    Extreme,
}

/// Realiza una limpieza agresiva de la memoria en Windows y Linux.
///
/// En Windows: Mueve páginas al archivo de paginación (EmptyWorkingSet).
/// En Linux: Fuerza a 'mimalloc' (Rust) y 'glibc' (GTK/System) a devolver memoria al Kernel.
/// 
/// Ahora mide el impacto real del "giro" mostrando cuánta memoria se liberó.
pub fn trim_memory() {
    trim_memory_with_level(TrimLevel::Normal);
}

/// Detecta si el sistema Linux tiene swap o zram disponible
fn has_swap_available() -> bool {
    // Verificar swap tradicional en /proc/swaps
    if let Ok(swaps) = std::fs::read_to_string("/proc/swaps") {
        let lines: Vec<&str> = swaps.lines().collect();
        // Ignorar la primera línea (headers), verificar si hay al menos una entrada
        if lines.len() > 1 {
            for line in lines.iter().skip(1) {
                if !line.trim().is_empty() && !line.starts_with('#') {
                    println!("[Swap] Detectado swap tradicional: {}", line.split_whitespace().next().unwrap_or("unknown"));
                    return true;
                }
            }
        }
    }
    
    // Verificar zram devices en /sys/block/zram*
    if let Ok(entries) = std::fs::read_dir("/sys/block") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if let Some(name_str) = name.to_str() {
                if name_str.starts_with("zram") {
                    // Verificar si el zram está activo (tiene tamaño > 0)
                    let disksize_path = entry.path().join("disksize");
                    if let Ok(size_str) = std::fs::read_to_string(disksize_path) {
                        if let Ok(size_bytes) = size_str.trim().parse::<u64>() {
                            if size_bytes > 0 {
                                println!("[Swap] Detectado zram activo: {} ({} bytes)", name_str, size_bytes);
                                return true;
                            }
                        }
                    }
                }
            }
        }
    }
    
    println!("[Swap] No se detectó swap o zram disponible");
    false
}

/// Verifica si debemos usar comportamiento Windows-style en Linux
fn should_use_windows_style() -> bool {
    cfg!(target_os = "linux") && {
        // Prioridad 1: Variable de entorno explícita
        if let Ok(mode) = std::env::var("RUSTALE_LINUX_SWAP_MODE") {
            return mode == "windows";
        }
        
        // Prioridad 2: Auto-detección si hay swap disponible
        if has_swap_available() {
            println!("[Swap] Auto-activando modo Windows-style (swap detectado)");
            return true;
        }
        
        false
    }
}

/// Fuerza a Linux a mover páginas a swap/zram (comportamiento similar a Windows)
fn force_linux_swap_behavior() {
    println!("[Swap] Forzando comportamiento similar a Windows en Linux...");
    
    unsafe {
        // 1. Forzar a mimalloc a liberar memoria agresivamente
        unsafe extern "C" {
            fn mi_collect(force: bool);
        }
        mi_collect(true);
        
        // 2. Forzar a glibc a liberar memoria
        unsafe extern "C" {
            fn malloc_trim(pad: usize) -> i32;
        }
        malloc_trim(0);
        
        // 3. Usar madvise para indicar al kernel que las páginas no son necesarias
        // Esto incentiva al kernel a mover las páginas a swap
        #[cfg(target_os = "linux")]
        {
            use libc::{madvise, MADV_DONTNEED};
            
            // Intentar liberar memoria del proceso actual usando madvise
            // Nota: Esto es experimental y puede no funcionar en todos los sistemas
            let result = madvise(
                std::ptr::null_mut(), 
                0, 
                MADV_DONTNEED
            );
            
            if result == 0 {
                println!("[Swap] madvise(MADV_DONTNEED) ejecutado exitosamente");
            } else {
                println!("[Swap] madvise falló (esto es normal en muchos sistemas)");
            }
        }
        
        // 4. Forzar sync para asegurar que los datos se escriban en disco
        #[cfg(target_os = "linux")]
        {
            unsafe extern "C" {
                fn sync();
            }
            sync();
        }
    }
    
    // 5. Opcional: Aumentar temporalmente la presión de memoria
    // Esto incentiva al kernel a usar swap más agresivamente
    if let Ok(_) = std::fs::write("/proc/sys/vm/vfs_cache_pressure", "100") {
        println!("[Swap] Aumentada vfs_cache_pressure temporalmente");
    }
}

/// Versión escalonada de trim_memory() con diferentes niveles de agresividad
pub fn trim_memory_with_level(level: TrimLevel) {
    // Medir memoria ANTES del giro
    let before = get_memory_usage();
    
    // Verificar si debemos usar comportamiento Windows-style en Linux
    let use_windows_style_linux = should_use_windows_style();
    
    if use_windows_style_linux {
        println!("[Swap] Modo Windows-style activado para Linux");
    }
    
    match level {
        TrimLevel::Normal => {
            // Comportamiento estándar original
            #[cfg(target_os = "windows")]
            {
                use windows_sys::Win32::System::ProcessStatus::K32EmptyWorkingSet;
                use windows_sys::Win32::System::Threading::GetCurrentProcess;
                unsafe {
                    let process = GetCurrentProcess();
                    K32EmptyWorkingSet(process);
                }
            }

            #[cfg(target_os = "linux")]
            {
                if use_windows_style_linux {
                    // Comportamiento similar a Windows
                    force_linux_swap_behavior();
                } else {
                    // Comportamiento Linux original
                    unsafe {
                        unsafe extern "C" {
                            fn mi_collect(force: bool);
                        }
                        mi_collect(true);

                        unsafe extern "C" {
                            fn malloc_trim(pad: usize) -> i32;
                        }
                        malloc_trim(0);
                    }
                }
            }
        }
        
        TrimLevel::Aggressive => {
            // Múltiples pasadas para máxima limpieza
            for _i in 0..3 {
                #[cfg(target_os = "windows")]
                {
                    use windows_sys::Win32::System::ProcessStatus::K32EmptyWorkingSet;
                    use windows_sys::Win32::System::Threading::GetCurrentProcess;
                    unsafe {
                        let process = GetCurrentProcess();
                        K32EmptyWorkingSet(process);
                    }
                }

                #[cfg(target_os = "linux")]
                {
                    if use_windows_style_linux {
                        // Comportamiento similar a Windows
                        force_linux_swap_behavior();
                    } else {
                        // Comportamiento Linux original
                        unsafe {
                            unsafe extern "C" {
                                fn mi_collect(force: bool);
                            }
                            mi_collect(true);

                            unsafe extern "C" {
                                fn malloc_trim(pad: usize) -> i32;
                            }
                            malloc_trim(0);
                        }
                    }
                }
                
                // Pequeña pausa entre pasadas
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        
        TrimLevel::Extreme => {
            // Modo servidor: todo lo posible + configuración adicional
            trim_memory_with_level(TrimLevel::Aggressive);
            
            // Intentar liberar cachés adicionales del sistema
            #[cfg(target_os = "linux")]
            {
                if use_windows_style_linux {
                    // Comportamiento extremo similar a Windows
                    force_linux_swap_behavior();
                    
                    // Intentos adicionales de liberación
                    unsafe {
                        unsafe extern "C" {
                            fn malloc_trim(pad: usize) -> i32;
                        }
                        malloc_trim(0);
                    }
                } else {
                    // Comportamiento Linux original
                    unsafe {
                        // Intentar liberar caché de directorios
                        unsafe extern "C" {
                            fn sync();
                        }
                        sync();
                        
                        // Otra pasada de malloc_trim por si acaso
                        unsafe extern "C" {
                            fn malloc_trim(pad: usize) -> i32;
                        }
                        malloc_trim(0);
                    }
                }
            }
        }
    }
    
    // Medir memoria DESPUÉS del giro y calcular impacto
    let after = get_memory_usage();
    
    if before > 0 && after > 0 && matches!(level, TrimLevel::Normal | TrimLevel::Aggressive | TrimLevel::Extreme) {
        let freed = before.saturating_sub(after);
        let freed_mb = freed as f64 / 1024.0 / 1024.0;
        let after_mb = after as f64 / 1024.0 / 1024.0;
        
        // Auto-escalado: contar giros débiles
        if freed_mb < WEAK_TRIM_THRESHOLD_MB {
            let weak_count = CONSECUTIVE_WEAK_TRIMS.fetch_add(1, Ordering::Relaxed) + 1;
            println!("[AUTO-ESCALA] ⚠️ Giro débil: {:.1} MB (< {} MB). Total débiles: {}", 
                freed_mb, WEAK_TRIM_THRESHOLD_MB, weak_count);
        } else {
            // Resetear contador si el giro fue efectivo
            CONSECUTIVE_WEAK_TRIMS.store(0, Ordering::Relaxed);
            println!("[AUTO-ESCALA] ✅ Giro efectivo: {:.1} MB. Reset contador de giros débiles", freed_mb);
        }
        
        let level_emoji = match level {
            TrimLevel::Normal => "🌀",
            TrimLevel::Aggressive => "🌪️",
            TrimLevel::Extreme => "💥",
        };
        
        if freed_mb > 0.1 { // Solo mostrar si liberamos más de 0.1 MB
            println!("[GIRO] {} Memoria liberada: {:.1} MB ({:.1} MB → {:.1} MB) [{:?}]", 
                level_emoji, freed_mb, before / 1024 / 1024, after_mb, level);
        }
    }
    
    // Log silencioso para debug interno si se requiere, pero evitamos spam en release.
    #[cfg(debug_assertions)]
    println!("[Memory] {:?} Trim execution completed.", level);
}

/// Obtiene estadísticas completas de memoria para monitoreo en tiempo real
pub fn get_memory_stats() -> MemoryStats {
    let current = get_memory_usage();
    let current_mb = current as f64 / 1024.0 / 1024.0;
    
    MemoryStats {
        current_mb: current_mb,
        auto_trims: AUTO_TRIM_COUNT.load(Ordering::Relaxed),
        last_activity: LAST_ACTIVITY.lock()
            .map(|instant| instant.elapsed())
            .unwrap_or(Duration::ZERO),
    }
}

/// Estadísticas de memoria para monitoreo
#[derive(Debug, Clone)]
pub struct MemoryStats {
    pub current_mb: f64,
    pub auto_trims: u64,
    pub last_activity: Duration,
}

impl MemoryStats {
    /// Devuelve una descripción formateada del estado actual
    pub fn format_status(&self) -> String {
        let weight_emoji = if self.current_mb < 50.0 {
            "🪶" // Ultra ligero
        } else if self.current_mb < 100.0 {
            "🕊️" // Ligero
        } else if self.current_mb < 200.0 {
            "🦅" // Normal
        } else if self.current_mb < 400.0 {
            "🦉" // Pesado
        } else {
            "🐘" // Muy pesado
        };
        
        let activity_status = if self.last_activity < Duration::from_secs(10) {
            "🟢 Activo"
        } else if self.last_activity < Duration::from_secs(30) {
            "🟡 Inactivo"
        } else {
            "🔴 Dormido"
        };
        
        format!("{} {:.1}MB | {} | Auto-giros: {}", 
            weight_emoji, self.current_mb, activity_status, self.auto_trims)
    }
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

/// Detecta si estamos ejecutando en Wayland (solo Linux)
/// Desactiva módulos custom de redimensionamiento y title bar si es true
#[cfg(target_os = "linux")]
pub fn is_wayland() -> bool {
    // Método 1: Variable de entorno WAYLAND_DISPLAY (más confiable)
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        return true;
    }
    
    // Método 2: Variable de entorno XDG_SESSION_TYPE
    if let Ok(session_type) = std::env::var("XDG_SESSION_TYPE") {
        if session_type.to_lowercase() == "wayland" {
            return true;
        }
    }
    
    // Método 3: Variable de entorno WAYLAND_SOCKET
    if std::env::var("WAYLAND_SOCKET").is_ok() {
        return true;
    }
    
    // Método 4: Verificar procesos de compositores Wayland (fallback)
    if let Ok(output) = std::process::Command::new("pgrep")
        .args(&["-f", "sway|weston|gnome-shell.*wayland|kwin_wayland"])
        .output()
    {
        if !output.stdout.is_empty() {
            return true;
        }
    }
    
    false
}
