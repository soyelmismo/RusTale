use anyhow::{Context, Result};
use regex::Regex;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::fs;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

pub async fn start_playit(root_dir: &PathBuf, client: &reqwest::Client) -> Result<()> {
    println!("--- Initializing Playit.gg Tunnel ---");

    let tools_dir = root_dir.join("tools").join("playit");
    fs::create_dir_all(&tools_dir).await?;

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

    let bin_path = tools_dir.join(bin_name);

    // 2. Descargar si no existe
    if !bin_path.exists() {
        println!("[Tunnel] Downloading Playit agent...");
        let resp = client.get(binary_url).send().await?;
        if !resp.status().is_success() {
            anyhow::bail!("Failed to download playit: {}", resp.status());
        }
        let bytes = resp.bytes().await?;
        fs::write(&bin_path, bytes).await?;

        // Permisos de ejecución en Linux
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
        .current_dir(&tools_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("Failed to start playit agent")?;

    let stdout = child.stdout.take().unwrap();
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();

    // 4. Parsear salida en tiempo real
    tokio::spawn(async move {
        let re_claim = Regex::new(r"https://playit\.gg/claim/[a-zA-Z0-9]+").unwrap();

        println!("\n=======================================================");
        println!("           PLAYIT.GG TUNNEL STATUS");
        println!("=======================================================\n");

        while let Ok(Some(line)) = lines.next_line().await {
            // Detectar URL de reclamación (primera vez)
            if let Some(mat) = re_claim.find(&line) {
                println!("\n>>> ACTION REQUIRED: Link this server to the internet:");
                println!(">>> {}\n", mat.as_str());
                // Opcional: Abrir navegador automáticamente
                if let Err(e) = open::that(mat.as_str()) {
                    eprintln!("Failed to open claim URL: {}", e);
                }
            }

            // Detectar dirección pública (cuando ya está configurado)
            if line.contains("tunnel running") || line.contains("allocated") {
                println!("[Tunnel] {}", line);
            }

            // Log general para debug
            // println!("[Playit] {}", line);
        }
    });

    Ok(())
}
