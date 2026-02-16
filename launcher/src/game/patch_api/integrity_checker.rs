use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::fs::File;
use std::io::Read;
use ed25519_dalek::VerifyingKey;
use tokio::task;

use super::PatchApiManager;
use sha2::{Sha256, Digest};

/// Integrity checker for patches and downloads using the new patch API system
#[derive(Clone)]
pub struct IntegrityChecker {
    api_manager: Arc<PatchApiManager>,
}

impl IntegrityChecker {
    pub fn new(api_manager: Arc<PatchApiManager>) -> Self {
        Self { api_manager }
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
            let mut file = File::open(&path)
                .context("Failed to open patch file for integrity check")?;
            
            let file_size = file.metadata()
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
                let bytes_read = file.read(&mut buffer)
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
                        
                        // Formatear porcentaje solo para el callback
                        let pct_str = format!("{:.0}%", progress * 100.0);
                        cb(progress, &format!("Calculating checksum... {}", pct_str));
                    }
                }
            }
            
            let hash = hasher.finalize();
            Ok::<String, anyhow::Error>(format!("{:x}", hash))
        }).await??; // Doble ? (uno para join error, otro para el result interno)
        
        // Reportar finalización inmediatamente después de volver al contexto async
        if let Some(callback) = &progress_callback {
            callback(1.0, "Checksum calculation completed");
        }
        
        Ok(result)
    }

    /// Verifies patch signature using Ed25519
    pub async fn verify_patch_signature(&self, patch_path: &PathBuf, signature_path: &PathBuf) -> Result<bool> {
        use ed25519_dalek::{Verifier, VerifyingKey, Signature};
        
        if !signature_path.exists() {
            return Ok(false);
        }
        
        if !patch_path.exists() {
            return Ok(false);
        }
        
        // Read signature file
        let signature_bytes = std::fs::read(signature_path)
            .context("Failed to read signature file")?;
        
        // Read patch file
        let patch_bytes = std::fs::read(patch_path)
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
    async fn load_trusted_public_key(&self) -> Result<VerifyingKey> {
        use ed25519_dalek::VerifyingKey;
        
        // Try to load from config directory first
        if let Some(config_dir) = crate::config::get_app_dir().to_str() {
            let pubkey_path = PathBuf::from(config_dir).join("trusted_public_key.bin");
            
            if pubkey_path.exists() {
                let pubkey_bytes = std::fs::read(&pubkey_path)
                    .context("Failed to read trusted public key")?;
                
                if pubkey_bytes.len() == 32 {
                    if let Ok(key) = VerifyingKey::from_bytes(&pubkey_bytes.try_into().unwrap()) {
                         return Ok(key);
                    }
                }
            }
        }
        
        // Fallback to embedded trusted key (for Hytale official patches)
        // This would be the official Hytale public key in a real implementation
        let trusted_key = [
            0x2d, 0x8d, 0x3c, 0x8a, 0x1b, 0xe5, 0x0f, 0x9c,
            0x4d, 0x2a, 0x7f, 0x9b, 0x3c, 0x8a, 0x1b, 0xe5,
            0x0f, 0x9c, 0x4d, 0x2a, 0x7f, 0x9b, 0x3c, 0x8a,
            0x1b, 0xe5, 0x0f, 0x9c, 0x4d, 0x2a, 0x7f, 0x9b,
        ];
        
        Ok(VerifyingKey::from_bytes(&trusted_key).unwrap())
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
        F: Fn(f64, &str) + Send + Sync + 'static,
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
        let actual_size = std::fs::metadata(patch_path)
            .context("Failed to get patch metadata")?
            .len();
        
        result.actual_size = Some(actual_size);
        
        if let Some(expected) = expected_size {
            if let Some(callback) = &progress_callback {
                callback(0.2, "Verifying file size...");
            }
            if actual_size != expected {
                result.valid = false;
                result.errors.push(format!("Size mismatch: expected {}, got {}", expected, actual_size));
            }
        }
        
        // Calculate checksum
        if let Some(callback) = &progress_callback {
            callback(0.0, "Calculating checksum...");
        }
        
        // Create a callback for checksum progress (0.0-1.0 range)
        let progress_callback = progress_callback.map(|cb| std::sync::Arc::new(cb));
        let progress_callback_clone = progress_callback.clone();
        let scaled_callback = move |pct: f64, msg: &str| {
            // Reportar el progreso real del checksum (0.0-1.0)
            if let Some(cb) = progress_callback_clone.as_ref() {
                cb(pct, msg);
            }
        };
        
        // CAMBIO IMPORTANTE: .await
        // verify_patch_integrity ahora se llama async
        match self.verify_patch_integrity(patch_path, Some(scaled_callback), cancel_token).await {
            Ok(checksum) => {
                result.checksum = Some(checksum.clone());
                result.checksum_valid = true;
                if let Some(callback) = &progress_callback {
                    callback(1.0, "Checksum calculated successfully");
                }
            }
            Err(e) => {
                result.valid = false;
                result.errors.push(format!("Failed to calculate checksum: {}", e));
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
                        result.errors.push("Signature verification failed".to_string());
                        if let Some(callback) = &progress_callback {
                            callback(0.9, "⚠️ Digital signature verification failed");
                        }
                    }
                }
                Err(e) => {
                    result.valid = false;
                    result.errors.push(format!("Failed to verify signature: {}", e));
                    if let Some(callback) = &progress_callback {
                        callback(0.9, &format!("❌ Signature verification error: {}", e));
                    }
                }
            }
        } else {
            if let Some(callback) = &progress_callback {
                callback(0.8, "No signature provided - skipping signature verification");
            }
        }
        
        if let Some(callback) = &progress_callback {
            if result.is_valid() {
                callback(1.0, "✅ Integrity verification completed");
            } else {
                callback(1.0, "❌ Integrity verification failed");
            }
        }
        
        Ok(result)
    }

    /// Performs quick integrity check (size and existence only)
    pub async fn quick_integrity_check(&self, patch_path: &PathBuf) -> Result<bool> {
        if !patch_path.exists() {
            return Ok(false);
        }
        
        let metadata = std::fs::metadata(patch_path)
            .context("Failed to get patch metadata")?;
        
        // Check if file has reasonable size (> 0 and < 10GB)
        let size = metadata.len();
        Ok(size > 0 && size < 10_000_000_000)
    }

    /// Validates patch file format
    pub fn validate_patch_format(&self, patch_path: &PathBuf) -> Result<FormatValidationResult> {
        let mut result = FormatValidationResult::new();
        
        if !patch_path.exists() {
            result.valid = false;
            result.errors.push("Patch file does not exist".to_string());
            return Ok(result);
        }
        
        let mut file = File::open(patch_path)
            .context("Failed to open patch file")?;
        
        // Read first few bytes to check format
        let mut header = [0; 16];
        let bytes_read = file.read(&mut header)
            .context("Failed to read patch header")?;
        
        if bytes_read < 4 {
            result.valid = false;
            result.errors.push("File too small to be a valid patch".to_string());
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
            result.valid = false;
            result.errors.push("Unknown patch format".to_string());
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
    pub warnings: Vec<String>,
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
            warnings: Vec::new(),
        }
    }
    
    pub fn is_valid(&self) -> bool {
        self.valid && self.errors.is_empty()
    }
    
    pub fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }
}

/// Result of format validation
#[derive(Debug, Clone)]
pub struct FormatValidationResult {
    pub valid: bool,
    pub format: Option<String>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl FormatValidationResult {
    pub fn new() -> Self {
        Self {
            valid: true,
            format: None,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }
    
    pub fn is_valid(&self) -> bool {
        self.valid && self.errors.is_empty()
    }
}
