use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};
use std::fs::File;
use std::io::Read;

pub const AGENT_URL: &str =
    "https://github.com/soyelmismo/hytale-auth-server/releases/latest/download/dualauth-agent.jar";

/// Validates that a JAR file is properly formatted and not corrupted
fn validate_jar_file(path: &PathBuf) -> anyhow::Result<()> {
    let mut file = File::open(path)?;
    let mut header = [0u8; 4];
    file.read_exact(&mut header)?;
    
    // JAR files have the same magic number as ZIP: 0x504B0304
    if header != [0x50, 0x4B, 0x03, 0x04] {
        anyhow::bail!("File is not a valid JAR/ZIP archive");
    }
    
    // Verify minimum size (empty JAR is suspicious)
    let metadata = std::fs::metadata(path)?;
    if metadata.len() < 1024 { // Minimum 1KB
        anyhow::bail!("JAR file is too small ({} bytes), possibly corrupted", metadata.len());
    }
    
    println!("[Agent] JAR validation passed: {} bytes", metadata.len());
    Ok(())
}

/// Ensures agent is available with validation and retry mechanism
pub async fn ensure_agent_async(
    root_dir: &PathBuf,
    progress_callback: &impl Fn(String, f64, String),
    cancel_token: Option<Arc<AtomicBool>>,
) -> anyhow::Result<PathBuf> {
    let paths = crate::game::paths::GamePaths::new(root_dir.clone());
    let agent_path = paths.dualauth_agent();
    
    if let Some(parent) = agent_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    
    // If file exists, validate it first
    if agent_path.exists() {
        progress_callback("agent".to_string(), 0.0, "Validating existing agent...".to_string());
        
        match validate_jar_file(&agent_path) {
            Ok(_) => {
                progress_callback("agent".to_string(), 100.0, "Agent is valid and ready".to_string());
                return Ok(agent_path);
            }
            Err(e) => {
                println!("[Agent] Existing agent validation failed: {}. Re-downloading...", e);
                let _ = tokio::fs::remove_file(&agent_path).await;
            }
        }
    }
    
    // Download with retries
    let max_retries = 3;
    for attempt in 1..=max_retries {
        if let Some(cancel) = &cancel_token {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                anyhow::bail!("Download cancelled");
            }
        }
        
        progress_callback(
            "agent".to_string(), 
            0.0, 
            format!("Downloading agent (attempt {}/{})...", attempt, max_retries)
        );
        
        match rustale_shared::download_file(
            AGENT_URL,
            &agent_path,
            |phase, pct, speed, _total, _downloaded, _eta, _step| {
                progress_callback(phase, pct, speed);
            },
            cancel_token.clone(),
        ).await {
            Ok(_) => {
                // Validate downloaded file
                match validate_jar_file(&agent_path) {
                    Ok(_) => {
                        progress_callback("agent".to_string(), 100.0, "Agent downloaded and validated".to_string());
                        return Ok(agent_path);
                    }
                    Err(e) => {
                        eprintln!("[Agent] Downloaded file validation failed (attempt {}): {}", attempt, e);
                        let _ = tokio::fs::remove_file(&agent_path).await;
                    }
                }
            }
            Err(e) => {
                eprintln!("[Agent] Download failed (attempt {}): {}", attempt, e);
            }
        }
        
        // Exponential backoff before retry
        if attempt < max_retries {
            let delay = std::time::Duration::from_secs(2_u64.pow(attempt as u32));
            tokio::time::sleep(delay).await;
        }
    }
    
    anyhow::bail!("Failed to download valid agent after {} attempts", max_retries)
}

pub async fn ensure_agent(
    root_dir: &PathBuf,
    progress_callback: &impl Fn(String, f64, String),
    cancel_token: Option<Arc<AtomicBool>>,
) -> anyhow::Result<PathBuf> {
    // Use the async version for all calls
    ensure_agent_async(root_dir, progress_callback, cancel_token).await
}
