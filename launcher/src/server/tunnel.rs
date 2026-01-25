use anyhow::{Context, Result};
use regex::Regex;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::fs;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

// Añadimos un canal 'on_ready' para avisar al runner
pub async fn start_playit(
    root_dir: &PathBuf,
    client: &reqwest::Client,
    on_ready: mpsc::Sender<String>,
) -> Result<()> {
    println!("--- Initializing Playit.gg Tunnel ---");

    let playit_dir = root_dir.join("tools").join("playit");
    fs::create_dir_all(&playit_dir).await?;

    // 1. Determinar ejecutable según SO
    let (binary_url, bin_name) = if cfg!(windows) {
        (
            "https://github.com/playit-cloud/playit-agent/releases/latest/download/playit-windows-x86_64.exe",
            "playit.exe",
        )
    } else {
        (
            "https://github.com/playit-cloud/playit-agent/releases/latest/download/playit-linux-x86_64",
            "playit",
        )
    };

    let bin_path = playit_dir.join(bin_name);

    // 2. Descargar si no existe
    if !bin_path.exists() {
        println!("[Tunnel] Downloading Playit agent...");
        let resp = client.get(binary_url).send().await?;
        if !resp.status().is_success() {
            anyhow::bail!("Failed to download playit: {}", resp.status());
        }
        let bytes = resp.bytes().await?;
        fs::write(&bin_path, bytes).await?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&bin_path).await?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&bin_path, perms).await?;
        }
        println!("[Tunnel] Download complete.");
    }

    // 3. Ejecutar agente
    println!("[Tunnel] Launching agent...");

    let mut child = Command::new(&bin_path)
        .arg("--stdout")
        .arg("--secret-path")
        .arg(&root_dir.join("tools").join("playit").join("secret.json"))
        .current_dir(&playit_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env("TERM", "dumb")
        .spawn()
        .context("Failed to start playit agent")?;

    let stdout = child.stdout.take().unwrap();
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();

    let re_url = Regex::new(r"https://playit\.gg/claim/[a-zA-Z0-9]+").unwrap();
    let mut notified = false;

    println!("\n=== PLAYIT MONITOR ACTIVE ===\n");

    while let Ok(Some(line)) = lines.next_line().await {
        // [DEBUG] Comentado para evitar flood de la consola si Playit usa TUI
        // println!("[Playit RAW] {}", line);

        // CASO 1: Encontrar URL de reclamo
        if let Some(mat) = re_url.find(&line) {
            let url = mat.as_str().to_string();

            if !notified {
                println!("\n>>> ACTION REQUIRED: {}", url);
                let _ = open::that(&url);
                let _ = on_ready.send(format!("SETUP REQUIRED: {}", url)).await;
                notified = true;
            }
        }

        // CASO 2: Túnel listo (busca palabras clave de playit v0.15+)
        if line.contains("tunnel running")
            || line.contains(".gl.joinmc.link")
            || line.contains(".ply.gg")
        {
            if !notified {
                let _ = on_ready.send("READY".to_string()).await;
                notified = true;
            }
        }
    }

    // If loop ends, child died or stdout closed
    let _ = child.kill().await;

    Ok(())
}
