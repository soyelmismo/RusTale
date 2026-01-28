use anyhow::{Context, Result};
use regex::Regex;
use std::io::Write;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::fs;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc; // Necesario para flush del stdout en la barra de progreso

pub async fn start_playit(
    root_dir: &PathBuf,
    client: &reqwest::Client,
    on_ready: mpsc::Sender<String>,
) -> Result<()> {
    println!("--- Initializing Playit.gg Tunnel ---");

    let playit_dir = root_dir.join("tools").join("playit");
    fs::create_dir_all(&playit_dir).await?;

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

    let mut need_download = !bin_path.exists();

    if bin_path.exists() {
        if let Ok(meta) = fs::metadata(&bin_path).await {
            if meta.len() == 0 {
                println!("[Tunnel] Binary found but empty (corrupt). Deleting...");
                let _ = fs::remove_file(&bin_path).await;
                need_download = true;
            }
        }
    }

    if need_download {
        println!("[Tunnel] Downloading Playit agent...");

        crate::game::downloader::download_file(
            client,
            binary_url,
            &bin_path,
            |pct, speed| {
                print!("\r[Tunnel] Downloading: {:.1}% ({})     ", pct, speed);
                let _ = std::io::stdout().flush();
            },
            None,
        )
        .await
        .context("Failed to download Playit agent")?;

        println!();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&bin_path).await?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&bin_path, perms).await?;
        }
        println!("[Tunnel] Download complete.");
    }

    // 4. Ejecutar agente
    println!("[Tunnel] Launching agent: {:?}", bin_path);

    let mut child = Command::new(&bin_path)
        .arg("--stdout")
        .arg("--secret_path")
        .arg(playit_dir.join("secret.json")) // Guardar secreto en la misma carpeta tools/playit
        .current_dir(&playit_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped()) // Capturamos STDERR para ver errores
        .kill_on_drop(true)
        .env("TERM", "dumb")
        .spawn()
        .context("Failed to spawn playit process")?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();

    let mut err_reader = BufReader::new(stderr).lines();

    tokio::spawn(async move {
        while let Ok(Some(line)) = err_reader.next_line().await {
            eprintln!("[Playit LOG] {}", line);
        }
    });

    let re_url = Regex::new(r"https://playit\.gg/claim/[a-zA-Z0-9]+").unwrap();
    let mut notified = false;

    println!("\n=== PLAYIT MONITOR ACTIVE ===\n");

    loop {
        tokio::select! {
            line_res = lines.next_line() => {
                match line_res {
                    Ok(Some(line)) => {
                        // CASO 1: URL de Reclamo (Primera vez)
                        if let Some(mat) = re_url.find(&line) {
                            let url = mat.as_str().to_string();

                            if !notified {
                                println!("\n>>> ACTION REQUIRED: {}", url);
                                println!(">>> Open that URL in your browser to link this server.\n");
                                let _ = open::that(&url);
                                let _ = on_ready.send(format!("SETUP REQUIRED: {}", url)).await;
                                notified = true;
                            }
                        }

                        if line.contains("tunnel running")
                            || line.contains(".gl.joinmc.link")
                            || line.contains(".ply.gg")
                        {
                            if !notified {
                                let _ = on_ready.send("READY".to_string()).await;
                                notified = true;
                            }
                            if line.contains("address") || line.contains("tunnel running") {
                                println!("[Playit] {}", line);
                            }
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        eprintln!("Error reading playit output: {}", e);
                        break;
                    }
                }
            }
            _ = child.wait() => {
                println!("Playit process exited.");
                break;
            }
        }
    }

    Ok(())
}
