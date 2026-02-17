use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};

pub const AGENT_URL: &str =
    "https://github.com/soyelmismo/hytale-auth-server/releases/latest/download/dualauth-agent.jar";

pub async fn ensure_agent(
    client: &reqwest::Client,
    root_dir: &PathBuf,
    progress_callback: &impl Fn(&str, f64, &str),
    cancel_token: Option<Arc<AtomicBool>>,
) -> anyhow::Result<PathBuf> {
    let paths = crate::game::paths::GamePaths::new(root_dir.clone());
    let agent_path = paths.dualauth_agent();

    if let Some(parent) = agent_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let mut needs_download = true;

    if agent_path.exists() {
        progress_callback("agent", 0.0, "Checking server agent version...");

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
                        progress_callback("agent", 100.0, "Agent is up to date");
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
        progress_callback("agent", 0.0, "Downloading Server Auth Agent...");

        crate::game::downloader::download_file(
            client,
            AGENT_URL,
            &agent_path,
            |pct, speed, total, downloaded, estimated_time| {
                let size_info = if total > 0 {
                    format!(
                        "{} / {}",
                        crate::game::patch_api::utils::format_bytes(downloaded),
                        crate::game::patch_api::utils::format_bytes(total)
                    )
                } else {
                    crate::game::patch_api::utils::format_bytes(downloaded)
                };

                let eta_info = if let Some(estimated_time) = &estimated_time {
                    format!(" • ETA: {}", estimated_time)
                } else {
                    String::new()
                };

                progress_callback(
                    "agent",
                    pct as f64,
                    &format!(
                        "Downloading Server Auth Agent... ({}{}{})",
                        speed, size_info, eta_info
                    ),
                );
            },
            cancel_token,
        )
        .await?;

        progress_callback("agent", 100.0, "Server Auth Agent downloaded");
    }

    Ok(agent_path)
}
