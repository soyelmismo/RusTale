use sha2::Digest;
use std::path::Path;
use std::sync::OnceLock;

static EXPECTED_CHECKSUM: OnceLock<&'static str> = OnceLock::new();

pub fn set_expected_checksum(checksum: &'static str) {
    let _ = EXPECTED_CHECKSUM.set(checksum);
}

/// Security guard to clean up temporary files when exiting the scope via Drop.
pub struct FileCleanupGuard {
    pub path: std::path::PathBuf,
}

impl Drop for FileCleanupGuard {
    fn drop(&mut self) {
        #[cfg(target_os = "windows")]
        if self.path.exists() {
            // Try to delete the file. Use std::fs sync because Drop cannot be async.
            // The file might be locked by the game process if it's still running.
            // We try multiple times with delays to handle the case where the game
            // is shutting down but hasn't released the DLL yet.
            let mut attempts = 0;
            loop {
                match std::fs::remove_file(&self.path) {
                    Ok(_) => {
                        println!("[Cleanup] Injected file deleted: {:?}", self.path);
                        break;
                    }
                    Err(e) => {
                        attempts += 1;
                        if attempts >= 5 {
                            // File is still locked after 5 attempts (500ms total)
                            // This is non-critical - the file will be cleaned up on next launch
                            eprintln!("[Cleanup] Could not delete {:?} after {} attempts: {}", self.path, attempts, e);
                            eprintln!("[Cleanup] File will be cleaned up on next launch");
                            break;
                        }
                        // Wait a bit for the game process to release the file
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
            }
        }
    }
}

pub fn verify_aurora_checksum(aurora_path: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    // Checksum embebido como variable de entorno en tiempo de compilación.
    // Si la variable no fue definida en tiempo de compilación (builds de desarrollo),
    // usamos el placeholder y saltamos la verificación.
    let embedded_checksum = EXPECTED_CHECKSUM.get().copied().or(option_env!("AURORA_CHECKSUM")).unwrap_or("dev_checksum_placeholder");

    // En modo dev (sin AURORA_CHECKSUM en compile-time) se acepta cualquier aurora
    // para no bloquear el flujo de desarrollo.
    if embedded_checksum == "dev_checksum_placeholder" {
        println!("[Security] AURORA_CHECKSUM not set at compile time — skipping checksum verification (dev mode).");
        return Ok(true);
    }

    // Calcular checksum del archivo actual
    let aurora_bytes = std::fs::read(aurora_path)?;
    let mut hasher = sha2::Sha256::new();
    hasher.update(&aurora_bytes);
    let current_checksum = format!("{:x}", hasher.finalize());

    let is_valid = embedded_checksum == current_checksum;

    if !is_valid {
        eprintln!("[Security] Aurora checksum mismatch!");
        eprintln!("[Security] Expected: {}", embedded_checksum);
        eprintln!("[Security] Current:  {}", current_checksum);
    } else {
        println!("[Security] Aurora checksum verified: {}", current_checksum);
    }

    Ok(is_valid)
}

pub fn ensure_aurora_installed() -> Result<(), String> {
    let tools_dir = rustale_shared::config::get_app_dir().join("tools");

    // Asegurar que el directorio tools exista
    if !tools_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&tools_dir) {
            return Err(format!("Failed to create tools directory: {}", e));
        }
    }

    let aurora_lib = format!("aurora{}", std::env::consts::DLL_SUFFIX);

    let aurora_path = tools_dir.join(&aurora_lib);

    // Paso 1: Verificar si Aurora ya existe en tools/ con el checksum correcto
    if aurora_path.exists() {
        match verify_aurora_checksum(&aurora_path) {
            Ok(true) => {
                println!("[Aurora] Found valid Aurora binary in tools/");
                return Ok(());
            }
            Ok(false) => {
                println!("[Aurora] Existing Aurora binary in tools/ has invalid checksum");
            }
            Err(e) => {
                return Err(format!("Failed to verify Aurora checksum in tools/: {}", e));
            }
        }
    }

    // Paso 2: Buscar Aurora junto al ejecutable y copiarlo si es válido
    let exe_path = match std::env::current_exe() {
        Ok(path) => path,
        Err(e) => return Err(format!("Cannot get executable path: {}", e)),
    };

    let exe_dir = match exe_path.parent() {
        Some(dir) => dir,
        None => return Err("Cannot get executable directory".to_string()),
    };

    let source_path = exe_dir.join(&aurora_lib);

    if source_path.exists() {
        println!("[Aurora] Found Aurora binary alongside executable, verifying...");
        match verify_aurora_checksum(&source_path) {
            Ok(true) => match std::fs::copy(&source_path, &aurora_path) {
                Ok(_) => {
                    println!("[Aurora] Copied valid Aurora binary to tools/");
                    return Ok(());
                }
                Err(e) => return Err(format!("Failed to copy Aurora to tools/: {}", e)),
            },
            Ok(false) => {
                println!("[Aurora] Aurora binary alongside executable has invalid checksum");
            }
            Err(e) => {
                return Err(format!(
                    "Failed to verify Aurora checksum alongside executable: {}",
                    e
                ));
            }
        }
    }

    // Paso 3: No se encontró Aurora válido en ninguna ubicación
    Err(format!(
        "Aurora binary not found or invalid. Expected locations:\n  - {:?}\n  - {:?}",
        aurora_path, source_path
    ))
}
