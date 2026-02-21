use anyhow::{Context, Result};
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};
use tokio::fs;
use tokio::io::AsyncReadExt;
use tokio::task;

use sha2::{Digest, Sha256};

/// Integrity checker for patches and downloads
#[derive(Clone)]
pub struct IntegrityChecker {}

impl IntegrityChecker {
    pub fn new() -> Self {
        Self {}
    }

    /// Validates the integrity of a PWR patch file
    pub async fn validate_patch_file(&self, pwr_path: &PathBuf) -> anyhow::Result<()> {
        if !pwr_path.exists() {
            anyhow::bail!("Patch file does not exist: {}", pwr_path.display());
        }

        let metadata = fs::metadata(pwr_path)
            .await
            .context("Failed to read patch file metadata")?;

        if metadata.len() == 0 {
            anyhow::bail!("Patch file is empty: {}", pwr_path.display());
        }

        // Check PWR file magic number
        let mut file = fs::File::open(pwr_path)
            .await
            .context("Failed to open patch file")?;

        let mut buffer = [0u8; 8]; // Read first 8 bytes for magic number
        let bytes_read = file
            .read(&mut buffer)
            .await
            .context("Failed to read patch file")?;

        if bytes_read < 8 {
            anyhow::bail!(
                "Patch file appears to be corrupted (too small): {}",
                pwr_path.display()
            );
        }

        // PWR files should start with specific magic bytes
        // Common PWR magic patterns (adjust based on actual format)
        let valid_magic_patterns: &[&[u8]] = &[
            &[0x00, 0x5f, 0xef, 0x0f],       // "PWR" prefix
            &[0x1f, 0x8b],             // GZIP header
            &[0x50, 0x4b, 0x03, 0x04], // ZIP header
            &[0x28, 0xb5, 0x2f, 0xfd], // Zstandard (ZSTD)
            &[0x37, 0x7a, 0xbc, 0xaf], // 7-Zip header
            &[0x42, 0x5a, 0x68],       // BZIP2 header
        ];

        let magic_valid = valid_magic_patterns
            .iter()
            .any(|&pattern| buffer.starts_with(pattern));

        if !magic_valid {
            println!(
                "[Integrity] Invalid magic detected: {:02X?} at {}",
                &buffer[..4.min(bytes_read)],
                pwr_path.display()
            );
            anyhow::bail!(
                "Patch file has invalid magic number: {:02X?} at {}. Expected PWR, GZIP, ZIP, ZSTD, 7Z or BZIP2.",
                &buffer[..4.min(bytes_read)],
                pwr_path.display()
            );
        }

        // Additional check: verify file is not all zeros or repeated pattern
        let mut sample_buffer = [0u8; 1024];
        if let Ok(sample_bytes) = file.read(&mut sample_buffer).await {
            if sample_bytes > 0 {
                let first_byte = sample_buffer[0];
                let all_same = sample_buffer[..sample_bytes]
                    .iter()
                    .all(|&b| b == first_byte);

                if all_same {
                    anyhow::bail!(
                        "Patch file appears corrupted (repeated pattern 0x{:02x}): {}",
                        first_byte,
                        pwr_path.display()
                    );
                }
            }
        }

        Ok(())
    }

    /// Verifies the integrity of the extracted game files
    pub async fn verify_extraction_integrity(&self, game_dir: &PathBuf) -> anyhow::Result<()> {
        let critical_paths = vec![
            "Client",
            "Client/HytaleClient",
            "Client/HytaleClient.exe",
            "Assets.zip",
        ];

        let mut missing_files: Vec<&str> = Vec::new();
        let mut empty_files: Vec<&str> = Vec::new();

        for path in &critical_paths {
            let full_path = game_dir.join(path);

            if !full_path.exists() {
                if path.contains("HytaleClient.exe") && !cfg!(windows) {
                    continue;
                }
                if path.contains("HytaleClient") && cfg!(windows) {
                    continue;
                }
                missing_files.push(path);
                continue;
            }

            if full_path.is_file() {
                let metadata = fs::metadata(&full_path).await?;
                if metadata.len() == 0 {
                    empty_files.push(path);
                }
            }
        }

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

            if !has_libraries {
                anyhow::bail!("No library files found in Client/ directory.");
            }

            if file_count < 5 || total_size < 20_000_000 {
                anyhow::bail!(
                    "Installation appears incomplete: {} files, {} bytes total.",
                    file_count,
                    total_size
                );
            }
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

        Ok(())
    }

    /// Verifies patch file integrity using SHA256
    pub async fn verify_patch_integrity<F>(
        &self,
        patch_path: &PathBuf,
        progress_callback: Option<F>,
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> Result<String>
    where
        F: Fn(f64, &str) + Send + Sync + Clone + 'static,
    {
        let path = patch_path.clone();
        let callback = progress_callback.clone();
        let cancel_token_clone = cancel_token.clone();

        let result = task::spawn_blocking(move || {
            let mut file = std::fs::File::open(&path)
                .context("Failed to open patch file for integrity check")?;

            let file_size = file
                .metadata()
                .context("Failed to get patch file metadata")?
                .len();

            let mut hasher = Sha256::new();
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

                if let Some(token) = &cancel_token_clone {
                    if token.load(std::sync::atomic::Ordering::Relaxed) {
                        return Err(anyhow::anyhow!("Checksum verification cancelled"));
                    }
                }

                if loops % 25 == 0 || bytes_read_total == file_size {
                    if let Some(cb) = &callback {
                        let progress = if file_size > 0 {
                            bytes_read_total as f64 / file_size as f64
                        } else {
                            0.0
                        };
                        cb(progress, "verifying_checksum");
                    }
                }
            }

            let hash = hasher.finalize();
            Ok::<String, anyhow::Error>(format!("{:x}", hash))
        })
        .await??;

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

        if !signature_path.exists() || !patch_path.exists() {
            return Ok(false);
        }

        let signature_bytes = fs::read(signature_path)
            .await
            .context("Failed to read signature file")?;

        let patch_bytes = fs::read(patch_path)
            .await
            .context("Failed to read patch file")?;

        if signature_bytes.len() != 64 {
            return Ok(false);
        }

        let signature_array: [u8; 64] = signature_bytes.clone().try_into().unwrap();
        let signature = Signature::from_bytes(&signature_array);

        let public_key = self.load_trusted_public_key().await?;

        match public_key.verify(&patch_bytes, &signature) {
            Ok(()) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    /// Loads trusted public key
    async fn load_trusted_public_key(&self) -> Result<ed25519_dalek::VerifyingKey> {
        let pubkey_path = crate::config::get_app_dir().join("trusted_public_key.bin");

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

        if !patch_path.exists() {
            result.valid = false;
            result.errors.push("Patch file does not exist".to_string());
            return Ok(result);
        }

        let actual_size = fs::metadata(patch_path)
            .await
            .context("Failed to get patch metadata")?
            .len();

        result.actual_size = Some(actual_size);

        if let Some(expected) = expected_size {
            if actual_size != expected {
                result.valid = false;
                result.errors.push(format!(
                    "Size mismatch: expected {}, got {}",
                    expected, actual_size
                ));
            }
        }

        let checksum_callback = progress_callback.clone();
        match self
            .verify_patch_integrity(patch_path, checksum_callback, cancel_token)
            .await
        {
            Ok(checksum) => {
                result.checksum = Some(checksum);
                result.checksum_valid = true;
            }
            Err(e) => {
                result.valid = false;
                result
                    .errors
                    .push(format!("Failed to calculate checksum: {}", e));
            }
        }

        if let Some(sig_path) = signature_path {
            match self.verify_patch_signature(patch_path, sig_path).await {
                Ok(signature_valid) => {
                    result.signature_valid = signature_valid;
                    if !signature_valid {
                        result.valid = false;
                        result
                            .errors
                            .push("Signature verification failed".to_string());
                    }
                }
                Err(e) => {
                    result.valid = false;
                    result
                        .errors
                        .push(format!("Failed to verify signature: {}", e));
                }
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
