use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};

pub const AGENT_URL: &str =
    "https://github.com/soyelmismo/hytale-auth-server/releases/latest/download/dualauth-agent.jar";

pub async fn ensure_agent(
    client: &rustale_shared::reqwest::Client,
    root_dir: &PathBuf,
    progress_callback: &impl Fn(String, f64, String),
    cancel_token: Option<Arc<AtomicBool>>,
) -> anyhow::Result<PathBuf> {
    let paths = crate::game::paths::GamePaths::new(root_dir.clone());
    let agent_path = paths.dualauth_agent();

    if let Some(parent) = agent_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let mut needs_download = true;

    if agent_path.exists() {
        progress_callback("agent".to_string(), 0.0, "Checking server agent version...".to_string());

        // Verificar por tamaño (HEAD request) con un timeout agresivo
        if let Ok(resp) = client
            .head(AGENT_URL)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
        {
            if resp.status().is_success() {
                let remote_size = resp.content_length().unwrap_or(0);
                if let Ok(meta) = tokio::fs::metadata(&agent_path).await {
                    if meta.len() == remote_size && remote_size > 0 {
                        progress_callback("agent".to_string(), 100.0, "Agent is up to date".to_string());
                        needs_download = false;
                    } else {
                        println!(
                            "[Agent] Size mismatch (Local: {}, Remote: {}). Updating...",
                            meta.len(),
                            remote_size
                        );
                    }
                }
            } else {
                println!(
                    "[Agent] Could not verify version (HTTP {}). Using local.",
                    resp.status()
                );
                needs_download = false;
            }
        } else {
            println!("[Agent] Network error checking version. Using local.");
            needs_download = false;
        }
    }

    if needs_download {
        progress_callback("agent".to_string(), 0.0, "Downloading Server Auth Agent...".to_string());

        rustale_shared::download_file(
            AGENT_URL,
            &agent_path,
            |phase, pct, speed, _total, _downloaded, _eta, _step| {
                progress_callback(phase, pct, speed);
            },
            cancel_token,
        )
        .await?;

        progress_callback("agent".to_string(), 100.0, "Server Auth Agent downloaded".to_string());
    }

    Ok(agent_path)
}
