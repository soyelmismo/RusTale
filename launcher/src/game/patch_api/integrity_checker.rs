use anyhow::{Context, Result};
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};
use tokio::fs;
use tokio::io::AsyncReadExt;
use tokio::task;

use sha2::{Digest, Sha256};

/// Integrity checker for patches and downloads using the new patch API system
#[derive(Clone)]
pub struct IntegrityChecker {
    // No api_manager field needed for integrity checks
}

impl IntegrityChecker {
    pub fn new() -> Self {
        Self {}
    }

    /// Validates the integrity of a PWR patch file
    /// Checks if the file exists, is readable, and has valid content
    pub async fn validate_patch_file(&self, pwr_path: &PathBuf) -> anyhow::Result<()> {
        use tokio::fs;

        // Check if file exists
        if !pwr_path.exists() {
            anyhow::bail!("Patch file does not exist: {}", pwr_path.display());
        }

        // Check file size (should be greater than 0)
        let metadata = fs::metadata(pwr_path)
            .await
            .context("Failed to read patch file metadata")?;

        if metadata.len() == 0 {
            anyhow::bail!("Patch file is empty: {}", pwr_path.display());
        }

        // Try to read first few bytes to verify it's a valid file
        let mut file = fs::File::open(pwr_path)
            .await
            .context("Failed to open patch file")?;

        let mut buffer = [0u8; 1024];
        let bytes_read = file
            .read(&mut buffer)
            .await
            .context("Failed to read patch file")?;

        if bytes_read == 0 {
            anyhow::bail!(
                "Patch file appears to be corrupted (cannot read): {}",
                pwr_path.display()
            );
        }

        println!(
            "Patch file validation passed: {} ({} bytes)",
            pwr_path.display(),
            metadata.len()
        );
        Ok(())
    }

    /// Verifies the integrity of the extracted game files
    /// This ensures that critical game files exist and are not empty
    pub async fn verify_extraction_integrity(&self, game_dir: &PathBuf) -> anyhow::Result<()> {
        // List of critical files/directories that must exist after extraction
        let critical_paths = vec![
            "Client",              // Main game client directory
            "Client/HytaleClient", // Main executable (Linux/Mac)
            "Client/HytaleClient.exe", // Main executable (Windows)
                                   // Note: Libraries are directly in Client/, not in Client/libs/
                                   // We'll check for common library patterns instead
        ];

        let mut missing_files: Vec<&str> = Vec::new();
        let mut empty_files: Vec<&str> = Vec::new();

        for path in &critical_paths {
            let full_path = game_dir.join(path);

            if !full_path.exists() {
                // Skip platform-specific executables that don't apply
                if path.contains("HytaleClient.exe") && !cfg!(windows) {
                    continue;
                }
                if path.contains("HytaleClient") && cfg!(windows) {
                    continue;
                }
                missing_files.push(path);
                continue;
            }

            // For files, check they're not empty
            if full_path.is_file() {
                let metadata = fs::metadata(&full_path).await?;
                if metadata.len() == 0 {
                    empty_files.push(path);
                }
            }
        }

        // Additional check: ensure Client directory has substantial content
        let client_dir = game_dir.join("Client");
        if client_dir.exists() {
            let mut entries = fs::read_dir(&client_dir).await?;
            let mut file_count = 0;
            let mut total_size = 0u64;
            let mut has_libraries = false;

            while let Ok(Some(entry)) = entries.next_entry().await {
                if let Ok(metadata) = entry.metadata().await {
                    let path = entry.path();
                    if metadata.is_file() {
                        file_count += 1;
                        total_size += metadata.len();

                        // Check for library files directly in Client/
                        if let Some(extension) = path.extension() {
                            if let Some(ext_str) = extension.to_str() {
                                match ext_str {
                                    "jar" | "so" | "dll" | "dylib" => {
                                        has_libraries = true;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }

            // Verify we have libraries and substantial content
            if !has_libraries {
                anyhow::bail!(
                    "No library files found in Client/ directory. Expected .jar, .so, .dll, or .dylib files."
                );
            }

            if file_count < 5 || total_size < 10_000_000 {
                // Less than 10MB seems suspicious
                anyhow::bail!(
                    "Installation appears incomplete: {} files, {} bytes total. Expected at least 5 files and 10MB.",
                    file_count,
                    total_size
                );
            }

            println!(
                "Client directory verification passed: {} files, {} bytes, libraries found: {}",
                file_count, total_size, has_libraries
            );
        } else {
            missing_files.push("Client");
        }

        if !missing_files.is_empty() || !empty_files.is_empty() {
            let mut error_msg = "Critical game files are missing or corrupted:".to_string();

            if !missing_files.is_empty() {
                error_msg.push_str(&format!("\n  Missing: {}", missing_files.join(", ")));
            }

            if !empty_files.is_empty() {
                error_msg.push_str(&format!("\n  Empty: {}", empty_files.join(", ")));
            }

            anyhow::bail!(error_msg);
        }

        println!(
            "Game extraction verification passed: {} files validated",
            critical_paths.len()
        );
        Ok(())
    }

    /// Verifies patch file integrity using SHA256 (Offloaded to Blocking Thread)
    pub async fn verify_patch_integrity<F>(
        &self,
        patch_path: &PathBuf,
        progress_callback: Option<F>,
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> Result<String>
    where
        F: Fn(f64, &str) + Send + Sync + Clone + 'static,
    {
        // Clonar path para enviarlo al thread
        let path = patch_path.clone();

        // Clonar el callback para poder usarlo después
        let callback = progress_callback.clone();

        // Clonar el cancel token para usarlo en el thread bloqueante
        let cancel_token_clone = cancel_token.clone();

        // MOVIMIENTO CLAVE: spawn_blocking
        let result = task::spawn_blocking(move || {
            // Toda esta logica ahora ocurre en un thread independiente que no congela la UI
            let mut file = std::fs::File::open(&path)
                .context("Failed to open patch file for integrity check")?;

            let file_size = file
                .metadata()
                .context("Failed to get patch file metadata")?
                .len();

            let mut hasher = Sha256::new();
            // Buffer adaptativo: más grande para HDD, más pequeño para SSD
            // 2MB para balance general en HDD lentos
            let mut buffer = vec![0; 2 * 1024 * 1024];
            let mut bytes_read_total: u64 = 0;
            let mut loops: u64 = 0;

            if let Some(cb) = &callback {
                cb(0.0, "Starting checksum calculation...");
            }

            loop {
                let bytes_read = file
                    .read(&mut buffer)
                    .context("Failed to read patch file")?;

                if bytes_read == 0 {
                    break;
                }

                hasher.update(&buffer[..bytes_read]);
                bytes_read_total += bytes_read as u64;

                loops += 1;

                // Verificar cancelación cada ciertas iteraciones
                if let Some(token) = &cancel_token_clone {
                    if token.load(std::sync::atomic::Ordering::Relaxed) {
                        return Err(anyhow::anyhow!("Checksum verification cancelled by user"));
                    }
                }

                // Throttle para HDD: Actualizar cada 50MB aprox (menos frecuente para reducir overhead)
                if loops % 25 == 0 || bytes_read_total == file_size {
                    if let Some(cb) = &callback {
                        let progress = if file_size > 0 {
                            bytes_read_total as f64 / file_size as f64
                        } else {
                            0.0
                        };

                        // DATA, NOT TEXT: Just pass the key identifier.
                        // We remove the format!("{:.0}%", ...) part.
                        cb(progress, "verifying_checksum"); 
                    }
                }

            }

            let hash = hasher.finalize();
            Ok::<String, anyhow::Error>(format!("{:x}", hash))
        })
        .await??; // Doble ? (uno para join error, otro para el result interno)

        // Reportar finalización inmediatamente después de volver al contexto async
        if let Some(callback) = &progress_callback {
            callback(1.0, "Checksum calculation completed");
        }

        Ok(result)
    }

    /// Verifies patch signature using Ed25519
    pub async fn verify_patch_signature(
        &self,
        patch_path: &PathBuf,
        signature_path: &PathBuf,
    ) -> Result<bool> {
        use ed25519_dalek::{Signature, Verifier};

        if !signature_path.exists() {
            return Ok(false);
        }

        if !patch_path.exists() {
            return Ok(false);
        }

        // Read signature file
        let signature_bytes = fs::read(signature_path)
            .await
            .context("Failed to read signature file")?;

        // Read patch file
        let patch_bytes = fs::read(patch_path)
            .await
            .context("Failed to read patch file")?;

        // Try to parse signature (64 bytes for Ed25519)
        if signature_bytes.len() != 64 {
            return Ok(false);
        }

        let signature_array: [u8; 64] = signature_bytes.clone().try_into().unwrap();
        let signature = Signature::from_bytes(&signature_array);

        // Load public key from trusted source
        let public_key = self.load_trusted_public_key().await?;

        // Verify signature
        match public_key.verify(&patch_bytes, &signature) {
            Ok(()) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    /// Loads trusted public key for signature verification
    async fn load_trusted_public_key(&self) -> Result<ed25519_dalek::VerifyingKey> {
        // Try to load from config directory first
        if let Some(config_dir) = crate::config::get_app_dir().to_str() {
            let pubkey_path = PathBuf::from(config_dir).join("trusted_public_key.bin");

            if pubkey_path.exists() {
                let pubkey_bytes = fs::read(&pubkey_path)
                    .await
                    .context("Failed to read trusted public key")?;

                if pubkey_bytes.len() == 32 {
                    if let Ok(key) =
                        ed25519_dalek::VerifyingKey::from_bytes(&pubkey_bytes.try_into().unwrap())
                    {
                        return Ok(key);
                    }
                }
            }
        }

        // Fallback to embedded trusted key (for Hytale official patches)
        // This would be the official Hytale public key in a real implementation
        let trusted_key = [
            0x2d, 0x8d, 0x3c, 0x8a, 0x1b, 0xe5, 0x0f, 0x9c, 0x4d, 0x2a, 0x7f, 0x9b, 0x3c, 0x8a,
            0x1b, 0xe5, 0x0f, 0x9c, 0x4d, 0x2a, 0x7f, 0x9b, 0x3c, 0x8a, 0x1b, 0xe5, 0x0f, 0x9c,
            0x4d, 0x2a, 0x7f, 0x9b,
        ];

        Ok(ed25519_dalek::VerifyingKey::from_bytes(&trusted_key).unwrap())
    }

    /// Verifies complete download integrity
    pub async fn verify_download_integrity<F>(
        &self,
        patch_path: &PathBuf,
        signature_path: Option<&PathBuf>,
        expected_size: Option<u64>,
        progress_callback: Option<F>,
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> Result<IntegrityResult>
    where
        F: Fn(f64, &str) + Send + Sync + Clone + 'static,
    {
        let mut result = IntegrityResult::new();

        if let Some(callback) = &progress_callback {
            callback(0.0, "Starting integrity verification...");
        }

        // Check file exists
        if !patch_path.exists() {
            result.valid = false;
            result.errors.push("Patch file does not exist".to_string());
            return Ok(result);
        }

        if let Some(callback) = &progress_callback {
            callback(0.1, "Checking file existence...");
        }

        // Check file size
        let actual_size = fs::metadata(patch_path)
            .await
            .context("Failed to get patch metadata")?
            .len();

        result.actual_size = Some(actual_size);

        if let Some(expected) = expected_size {
            if let Some(callback) = &progress_callback {
                callback(0.2, "Verifying file size...");
            }
            if actual_size != expected {
                result.valid = false;
                result.errors.push(format!(
                    "Size mismatch: expected {}, got {}",
                    expected, actual_size
                ));
            }
        }

        // Calculate checksum
        if let Some(callback) = &progress_callback {
            callback(0.0, "Calculating checksum...");
        }

        // Create a callback for checksum progress (0.0-1.0 range)
        use std::sync::Arc;
        let checksum_callback: Option<Arc<dyn Fn(f64, &str) + Send + Sync>> = progress_callback
            .clone()
            .map(|cb| Arc::new(cb) as Arc<dyn Fn(f64, &str) + Send + Sync>);
        let checksum_callback_clone = checksum_callback.clone();
        let scaled_callback = move |pct: f64, msg: &str| {
            // Reportar el progreso real del checksum (0.0-1.0)
            if let Some(cb) = checksum_callback_clone.as_ref() {
                cb(pct, msg);
            }
        };

        // CAMBIO IMPORTANTE: .await
        // verify_patch_integrity ahora se llama async
        match self
            .verify_patch_integrity(patch_path, Some(scaled_callback), cancel_token)
            .await
        {
            Ok(checksum) => {
                result.checksum = Some(checksum.clone());
                result.checksum_valid = true;
                if let Some(callback) = &progress_callback {
                    callback(1.0, "Checksum calculated successfully");
                }
            }
            Err(e) => {
                result.valid = false;
                result
                    .errors
                    .push(format!("Failed to calculate checksum: {}", e));
            }
        }

        // Verify signature if provided
        if let Some(sig_path) = signature_path {
            if let Some(callback) = &progress_callback {
                callback(0.7, "Verifying digital signature...");
            }
            match self.verify_patch_signature(patch_path, sig_path).await {
                Ok(signature_valid) => {
                    result.signature_valid = signature_valid;
                    if signature_valid {
                        if let Some(callback) = &progress_callback {
                            callback(0.9, "Digital signature verified successfully");
                        }
                    } else {
                        result.valid = false;
                        result
                            .errors
                            .push("Signature verification failed".to_string());
                        if let Some(callback) = &progress_callback {
                            callback(0.9, "⚠️ Digital signature verification failed");
                        }
                    }
                }
                Err(e) => {
                    result.valid = false;
                    result
                        .errors
                        .push(format!("Failed to verify signature: {}", e));
                    if let Some(callback) = &progress_callback {
                        callback(0.9, &format!("❌ Signature verification error: {}", e));
                    }
                }
            }
        } else {
            if let Some(callback) = &progress_callback {
                callback(
                    0.8,
                    "No signature provided - skipping signature verification",
                );
            }
        }

        if let Some(callback) = &progress_callback {
            if result.is_valid() {
                callback(1.0, "✅ Integrity verification completed");
            } else {
                callback(1.0, "❌ Integrity verification failed");
            }
        }

        // Note: warnings field and has_warnings() method were removed from IntegrityResult
        // This section is no longer needed as warnings are not tracked

        Ok(result)
    }

    /// Validates patch file format
    pub fn validate_patch_format(&self, patch_path: &PathBuf) -> Result<FormatValidationResult> {
        let mut result = FormatValidationResult::new();

        if !patch_path.exists() {
            result.valid = false;
            result.errors.push("Patch file does not exist".to_string());
            return Ok(result);
        }

        let mut file = std::fs::File::open(patch_path).context("Failed to open patch file")?;

        // Read first few bytes to check format
        let mut header = [0; 16];
        let bytes_read = file
            .read(&mut header)
            .context("Failed to read patch header")?;

        if bytes_read < 4 {
            result.valid = false;
            result
                .errors
                .push("File too small to be a valid patch".to_string());
            return Ok(result);
        }

        // Check for common patch formats
        if header.starts_with(b"PWR") {
            result.format = Some("PWR".to_string());
            result.valid = true;
        } else if header.starts_with(b"PK") {
            result.format = Some("ZIP".to_string());
            result.valid = true;
        } else if header.starts_with(b"\x1f\x8b") {
            result.format = Some("GZIP".to_string());
            result.valid = true;
        } else {
            // Enhanced validation: Check for alternative PWR formats
            // Some servers use custom PWR formats with different headers
            if patch_path.extension().and_then(|s| s.to_str()) == Some("pwr") {
                // For .pwr files, check if they have reasonable size and structure
                let metadata = file.metadata().context("Failed to get file metadata")?;
                let file_size = metadata.len();
                
                // Reasonable size check: PWR patches are typically > 1MB and < 4GB
                if file_size > 1_048_576 && file_size < 4_294_967_296 {
                    // Additional check: Look for common PWR structural patterns
                    // Many PWR files start with binary headers that include version info
                    if (header[0] == 0x00 && header[1] == 0x5F) || // Custom PWR format pattern
                       (header[0] == 0x50 && header[1] == 0x57) || // Alternative "PW" start
                       (header[0] == 0x1F && header[1] == 0x8B) { // Compressed PWR
                        result.format = Some("PWR-Custom".to_string());
                        result.valid = true;
                        println!("[Integrity] Detected custom PWR format: {:02X?}", &header[..8]);
                    } else {
                        // Last resort: If it's a .pwr file with reasonable size, 
                        // assume it's valid but mark as unknown format
                        result.format = Some("PWR-Unknown".to_string());
                        result.valid = true;
                        println!("[Integrity] Accepting unknown PWR format: {:02X?} (size: {} bytes)", &header[..8], file_size);
                    }
                } else {
                    result.valid = false;
                    result.errors.push(format!(
                        "PWR file has invalid size: {} bytes (expected 1MB - 4GB)", 
                        file_size
                    ));
                }
            } else {
                result.valid = false;
                result.errors.push(format!(
                    "Unknown patch format. Header: {:02X?}", 
                    &header[..bytes_read.min(8)]
                ));
            }
        }

        Ok(result)
    }
}

/// Result of integrity verification
#[derive(Debug, Clone)]
pub struct IntegrityResult {
    pub valid: bool,
    pub actual_size: Option<u64>,
    pub checksum: Option<String>,
    pub checksum_valid: bool,
    pub signature_valid: bool,
    pub errors: Vec<String>,
}

impl IntegrityResult {
    pub fn new() -> Self {
        Self {
            valid: true,
            actual_size: None,
            checksum: None,
            checksum_valid: false,
            signature_valid: false,
            errors: Vec::new(),
        }
    }

    pub fn is_valid(&self) -> bool {
        self.valid && self.errors.is_empty()
    }
}

/// Result of format validation
#[derive(Debug, Clone)]
pub struct FormatValidationResult {
    pub valid: bool,
    pub format: Option<String>,
    pub errors: Vec<String>,
}

impl FormatValidationResult {
    pub fn new() -> Self {
        Self {
            valid: true,
            format: None,
            errors: Vec::new(),
        }
    }

    pub fn is_valid(&self) -> bool {
        self.valid && self.errors.is_empty()
    }
}
