use anyhow::{Context, Result};
use std::sync::{Arc, atomic::AtomicBool};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::AsyncBufReadExt;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GameVersionInfo {
    pub user_version: i32,
    pub current_local: i32,
    pub latest_remote: i32,
    pub available_versions: Vec<i32>,
    pub update_available: bool,
}

/// Implementation of the version search algorithm
pub async fn get_version_manifest(
    client: &reqwest::Client,
    channel: &str,
    base_dir: &std::path::PathBuf,
    user_version: i32,
) -> Result<GameVersionInfo> {
    let local_version = crate::game::install::get_local_version(base_dir, channel)
        .await
        .unwrap_or(0);
    let latest = find_latest_version(client, channel, Some(local_version)).await?;

    // Generates a list of available versions (from 1 to latest)
    let mut available: Vec<i32> = (1..=latest).collect();
    available.reverse(); // From newest to oldest for the UI

    Ok(GameVersionInfo {
        user_version,
        current_local: local_version,
        latest_remote: latest,
        available_versions: available,
        // update if user uses 0 (latest) and its local installed version is lower than latest remote
        update_available: user_version == 0 && local_version < latest,
    })
}

/// Installs Butler if not already installed
pub async fn install_butler(
    client: &reqwest::Client,
    base_dir: &PathBuf,
    progress_callback: impl Fn(&str, f64, &str),
    cancel_token: Option<Arc<AtomicBool>>,
) -> Result<PathBuf> {
    let paths = crate::game::paths::GamePaths::new(base_dir.clone());
    let tools_dir = base_dir.join("tools").join("butler");
    let butler_path = paths.butler();
    tokio::fs::create_dir_all(&tools_dir).await?;

    // Check if already installed
    if butler_path.exists() {
        let _ = crate::util::make_executable(&butler_path).await;
        progress_callback("butler", 100.0, "Butler already installed");
        return Ok(butler_path);
    }

    // Determine download URL based on OS
    let url = match std::env::consts::OS {
        "windows" => "https://broth.itch.zone/butler/windows-amd64/LATEST/archive/default",
        "macos" => "https://broth.itch.zone/butler/darwin-amd64/LATEST/archive/default",
        "linux" => "https://broth.itch.zone/butler/linux-amd64/LATEST/archive/default",
        _ => anyhow::bail!("Unsupported OS for Butler"),
    };

    progress_callback("butler", 0.0, "Downloading Butler...");

    let zip_path = tools_dir.join("butler.zip");

    // Download with automatic fallback
    download_with_fallback(
        client,
        url,
        &zip_path,
        |pct, speed| progress_callback("butler", pct as f64, &format!("Downloading Butler... ({})", speed)),
        cancel_token,
        |fallback_data| crate::game::fallback::get_butler_url(fallback_data),
        "Butler",
    ).await?;

    progress_callback("butler", 70.0, "Extracting Butler...");

    // Extract using spawn_blocking to avoid UI freeze
    let zip_path_clone = zip_path.clone();
    let tools_dir_clone = tools_dir.clone();

    tokio::task::spawn_blocking(move || -> Result<()> {
        let file = std::fs::File::open(&zip_path_clone).context("Failed to open Butler archive")?;
        let mut archive = zip::ZipArchive::new(file).context("Failed to read Butler archive")?;
        archive
            .extract(&tools_dir_clone)
            .context("Failed to extract Butler")?;
        Ok(())
    })
    .await
    .context("Butler extraction task failed")??;

    // Make executable on Unix
    let _ = crate::util::make_executable(&butler_path).await;

    // Cleanup
    let _ = tokio::fs::remove_file(&zip_path).await;

    progress_callback("butler", 100.0, "Butler installed");

    Ok(butler_path)
}

/// Finds the latest available game version from the server
/// Optimized with speculative parallelism and hinting
pub async fn find_latest_version(
    client: &reqwest::Client,
    channel: &str,
    start_hint: Option<i32>,
) -> Result<i32> {
    let os_name = std::env::consts::OS;
    let arch = get_arch_name();

    // Helper to check if a version exists on the server
    let version_exists = |version| {
        let url = format!(
            "https://game-patches.hytale.com/patches/{}/{}/{}/0/{}.pwr",
            os_name, arch, channel, version
        );
        let client = client.clone();
        async move {
            match client.head(&url).send().await {
                Ok(resp) => resp.status().is_success(),
                Err(_) => false,
            }
        }
    };

    let mut found_base = 0;

    // --- STEP 0: HINTING ---
    if let Some(hint) = start_hint {
        if hint > 0 {
            if version_exists(hint).await {
                found_base = hint;
                println!("Starting from hint: version {}", hint);
            }
        }
    }

    // --- STEP 1: PARALLEL SHORTCUTS ---
    if found_base == 0 {
        let shortcuts = vec![3000, 2000, 1000, 500, 250, 100, 50, 25, 10, 5, 1];
        println!("Probing version shortcuts in parallel...");
        
        let mut futures = Vec::new();
        for &v in &shortcuts {
            futures.push(async move { (v, version_exists(v).await) });
        }
        
        let results = futures::future::join_all(futures).await;
        for (v, exists) in results {
            if exists && v > found_base {
                found_base = v;
            }
        }
        
        if found_base > 0 {
            println!("Found base version via parallel probe: {}", found_base);
        }
    }

    if found_base == 0 {
        // Try fallback API
        println!("Main server failed, trying fallback API...");
        match crate::game::fallback::fetch_fallback_data(client).await {
            Ok(fallback_data) => {
                match crate::game::fallback::get_latest_version(&fallback_data, channel) {
                    Ok(version) => {
                        println!("Found latest version via fallback: {}", version);
                        return Ok(version);
                    }
                    Err(e) => {
                        println!("Fallback API failed to get version: {}", e);
                    }
                }
            }
            Err(e) => {
                println!("Failed to fetch fallback data: {}", e);
            }
        }

        // Double check if at least version 1 exists to ensure server is reachable
        if version_exists(1).await {
            found_base = 1;
        } else {
            anyhow::bail!(
                "Cannot reach game server or no versions available for {}/{}",
                os_name,
                arch
            );
        }
    }

    // --- PHASE 1: SPECULATIVE EXPONENTIAL SEARCH ---
    let mut lower = found_base;
    let mut step = 1;
    let mut upper_bound = found_base + 100; // Safe default
    
    println!("Exponential Search Phase (Speculative)...");
    loop {
        // Speculatively check next 3 powers of 2 jumps in parallel
        let jumps = vec![step, step * 2 + step, step * 4 + step * 2 + step];
        let mut futures = Vec::new();
        for &j in &jumps {
            futures.push(async move { (j, version_exists(lower + j).await) });
        }
        
        let results = futures::future::join_all(futures).await;
        
        let mut highest_jump = 0;
        let mut first_fail = 0;
        for (j, exists) in results {
            if exists { 
                highest_jump = j; 
            } else if first_fail == 0 {
                first_fail = j;
            }
        }
        
        if highest_jump == 0 {
            // All immediate jumps failed, the bound is somewhere in [lower, lower + jumps[0]]
            upper_bound = lower + jumps[0];
            break;
        } else {
            lower += highest_jump;
            if first_fail > 0 {
                // We found a transition: latest is in [lower, lower + (first_fail - highest_jump)]
                upper_bound = lower + (first_fail - highest_jump);
                break;
            }
            // All jumps succeeded, increase velocity and continue
            step *= 4; 
            println!("Found version {}, jumping further...", lower);
        }
        
        if lower > 10000 { break; }
    }

    // --- PHASE 2: PARALLEL BINARY SEARCH (4-WAY) ---
    let mut left = lower;
    let mut right = upper_bound;
    let mut latest = lower;

    println!("Refining search in range [{}, {}] (4-Way Parallel)...", left, right);
    
    while left <= right {
        if right - left < 4 {
            // Very small range, just linear check in parallel
            let mut futures = Vec::new();
            // FIX: Must include 'left' because after left = best_mid + 1, 'left' is unconfirmed
            for v in left..=right {
                if v > latest { // Avoid re-checking if we already know 'latest'
                    futures.push(async move { (v, version_exists(v).await) });
                }
            }
            if !futures.is_empty() {
                let results = futures::future::join_all(futures).await;
                for (v, exists) in results {
                    if exists && v > latest { latest = v; }
                }
            }
            break;
        }

        // Split the range into 4 segments and check the 3 mid-points
        let d = (right - left) / 4;
        let p1 = left + d;
        let p2 = left + 2 * d;
        let p3 = left + 3 * d;
        
        let m_points = vec![p1, p2, p3];
        let mut futures = Vec::new();
        for &p in &m_points {
            futures.push(async move { (p, version_exists(p).await) });
        }
        
        let results = futures::future::join_all(futures).await;
        
        let mut best_mid = 0;
        for (p, exists) in results {
            if exists { best_mid = p; }
        }

        if best_mid != 0 {
            latest = best_mid;
            left = best_mid + 1;
        } else {
            // All mid-points failed, so it must be below p1
            right = p1 - 1;
        }
    }

    println!("Latest version found: {}", latest);
    Ok(latest)
}

/// Downloads a server patch file with automatic fallback
pub async fn download_server_pwr(
    client: &reqwest::Client,
    channel: &str,
    target_version: i32,
    dest: &PathBuf,
    progress_callback: impl Fn(f32, &str),
    cancel_token: Option<Arc<AtomicBool>>,
) -> anyhow::Result<()> {
    let os = std::env::consts::OS;
    let arch = "amd64";
    let url = format!(
        "https://game-patches.hytale.com/patches/{}/{}/{}/0/{}.pwr",
        os, arch, channel, target_version
    );
    
    download_with_fallback(
        client,
        &url,
        dest,
        progress_callback,
        cancel_token,
        |fallback_data| crate::game::fallback::get_version_url(fallback_data, channel, target_version),
        "server PWR file",
    ).await
}

/// Downloads a file with automatic fallback to alternative API
pub async fn download_with_fallback<F>(
    client: &reqwest::Client,
    primary_url: &str,
    dest: &PathBuf,
    progress_callback: impl Fn(f32, &str),
    cancel_token: Option<Arc<AtomicBool>>,
    fallback_url_resolver: F,
    file_type: &str,
) -> anyhow::Result<()>
where
    F: FnOnce(&crate::game::fallback::FallbackAPI) -> anyhow::Result<String>,
{
    // Try primary download first
    let download_result = crate::game::downloader::download_file(
        client,
        primary_url,
        dest,
        |pct, speed, total, downloaded, eta| {
                let size_info = if total > 0 {
                    format!("{} / {}", 
                        crate::game::downloader::format_bytes(downloaded), 
                        crate::game::downloader::format_bytes(total)
                    )
                } else {
                    crate::game::downloader::format_bytes(downloaded)
                };
                
                let eta_info = if let Some(eta_str) = &eta {
                    format!(" • ETA: {}", eta_str)
                } else {
                    String::new()
                };
                
                progress_callback(pct, &format!("{}{}{}", speed, size_info, eta_info));
            },
        cancel_token.clone(),
    )
    .await;

    // If primary fails, try fallback
    if download_result.is_err() {
        println!("Main {} download failed, trying fallback API...", file_type);
        match crate::game::fallback::fetch_fallback_data(client).await {
            Ok(fallback_data) => {
                match fallback_url_resolver(&fallback_data) {
                    Ok(fallback_url) => {
                        println!("Downloading {} from fallback: {}", file_type, fallback_url);
                        crate::game::downloader::download_file(
                            client,
                            &fallback_url,
                            dest,
                            |pct, speed, total, downloaded, eta| {
                let size_info = if total > 0 {
                    format!("{} / {}", 
                        crate::game::downloader::format_bytes(downloaded), 
                        crate::game::downloader::format_bytes(total)
                    )
                } else {
                    crate::game::downloader::format_bytes(downloaded)
                };
                
                let eta_info = if let Some(eta_str) = &eta {
                    format!(" • ETA: {}", eta_str)
                } else {
                    String::new()
                };
                
                progress_callback(pct, &format!("{}{}{}", speed, size_info, eta_info));
            },
                            cancel_token,
                        )
                        .await?;
                    }
                    Err(e) => {
                        println!("Fallback API failed to get {} URL: {}", file_type, e);
                        return download_result; // Return original error
                    }
                }
            }
            Err(e) => {
                println!("Failed to fetch fallback data: {}", e);
                return download_result; // Return original error
            }
        }
    }

    Ok(())
}

/// Downloads a PWR patch file with automatic fallback
pub async fn download_pwr(
    client: &reqwest::Client,
    channel: &str,
    prev_version: i32,
    target_version: i32,
    progress_callback: &impl Fn(&str, f64, &str),
    cancel_token: Option<Arc<AtomicBool>>,
) -> anyhow::Result<PathBuf> {
    let cache_dir = crate::config::get_cache_dir("patches").await;
    let os_name = std::env::consts::OS;
    let arch = get_arch_name();

    let file_name = format!("{}-{}.pwr", prev_version, target_version);
    let dest = cache_dir.join(&file_name);

    let url_remote = format!(
        "https://game-patches.hytale.com/patches/{}/{}/{}/{}/{}.pwr",
        os_name, arch, channel, prev_version, target_version
    );

    progress_callback(
        "download",
        10.0,
        &format!(
            "Downloading update ({} -> {})...",
            prev_version, target_version
        ),
    );

    // Download with automatic fallback
    download_with_fallback(
        client,
        &url_remote,
        &dest,
        |pct, speed| {
            progress_callback(
                "download",
                pct as f64,
                &format!("Downloading patch... ({})", speed),
            );
        },
        cancel_token,
        |fallback_data| crate::game::fallback::get_version_url(fallback_data, channel, target_version),
        "PWR file",
    ).await?;

    progress_callback("download", 40.0, "PWR file downloaded");

    Ok(dest)
}

/// Applies a PWR patch file using butler
pub async fn apply_pwr(
    base_dir: &PathBuf,
    channel: &str,
    pwr_file: &PathBuf,
    install_dir_name: &str,
    progress_callback: &impl Fn(&str, f64, &str),
) -> anyhow::Result<()> {
    let game_install_dir = base_dir.join(channel).join(install_dir_name);
    let staging_dir = base_dir.join(channel).join("staging-temp");

    // Ensure target directory exists (Butler requirement)
    if !game_install_dir.exists() {
        tokio::fs::create_dir_all(&game_install_dir).await?;
    }

    // Clean staging directory if it exists to avoid conflicts
    if staging_dir.exists() {
        let _ = tokio::fs::remove_dir_all(&staging_dir).await;
    }
    tokio::fs::create_dir_all(&staging_dir).await?;

    let paths = crate::game::paths::GamePaths::new(base_dir.clone());
    let butler_path = paths.butler();

    // Ensure logs directory exists
    let logs_dir = base_dir.join("logs");
    let _ = tokio::fs::create_dir_all(&logs_dir).await;
    let log_file_path = logs_dir.join("butler_apply.log");
    let log_file = std::fs::File::create(&log_file_path)?;
    let mut log_writer = std::io::BufWriter::new(log_file);
    use std::io::Write;

    let mut cmd = tokio::process::Command::new(butler_path);

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000);
    }

    let mut child = cmd
        .arg("apply")
        .arg("--staging-dir")
        .arg(&staging_dir)
        .arg(pwr_file)
        .arg(&game_install_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let mut reader = tokio::io::BufReader::new(stdout);
    let mut err_reader = tokio::io::BufReader::new(stderr).lines();

    // Spawn a task to handle stderr
    tokio::spawn(async move {
        while let Ok(Some(line)) = err_reader.next_line().await {
            eprintln!("[Butler Error] {}", line);
        }
    });

    let mut current_pct = 0.0;
    let mut line_buf = Vec::new();

    // Use read_until(b'\r') because Butler uses \r to update progress without newlines
    while let Ok(n) = reader.read_until(b'\r', &mut line_buf).await {
        if n == 0 {
            break;
        }

        let raw_s = String::from_utf8_lossy(&line_buf);
        // Split by \n just in case Butler mixes them
        for line in raw_s.split('\n') {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            // Log to file
            let _ = writeln!(log_writer, "{}", line);
            let _ = log_writer.flush();

            if line.contains('%') {
                if let Some(pct) = parse_butler_line(line) {
                    current_pct = pct as f64;
                    let file = line.split('%').last().unwrap_or("").trim();
                    progress_callback("install", current_pct, file);
                }
            } else {
                if line.len() < 100 && !line.starts_with("\u{2590}") {
                    progress_callback("install", current_pct, line);
                }
            }
        }
        line_buf.clear();
    }

    let status = child.wait().await?;
    if !status.success() {
        // --- NEW: Recovery logic ---
        eprintln!(
            "Butler failed. Possible corrupt patch file. Deleting: {:?}",
            pwr_file
        );

        // If Butler fails, the .pwr file probably got corrupted (or incomplete if we didn't have atomic downloads).
        // We delete it to force a new download next time.
        if pwr_file.exists() {
            let _ = tokio::fs::remove_file(pwr_file).await;
        }

        // Clean up staging also
        let _ = tokio::fs::remove_dir_all(&staging_dir).await;

        anyhow::bail!("Butler failed to apply patch with status: {}", status);
    }

    // Clean up staging after success
    let _ = tokio::fs::remove_dir_all(&staging_dir).await;

    Ok(())
}

fn parse_butler_line(line: &str) -> Option<f32> {
    // Search for the percentage before the '%' symbol
    let parts: Vec<&str> = line.split('%').collect();
    if let Some(first_part) = parts.get(0) {
        let words: Vec<&str> = first_part.split_whitespace().collect();
        if let Some(last_word) = words.last() {
            return last_word.parse::<f32>().ok();
        }
    }
    None
}

fn get_arch_name() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    }
}

pub async fn clean_patches_cache(progress_callback: &impl Fn(&str, f64, &str)) -> Result<()> {
    let patches_cache_dir = crate::config::get_cache_dir("game_patches").await;

    if patches_cache_dir.exists() {
        progress_callback("cleanup", 0.0, "Cleaning patches cache...");

        // Delete all .pwr files in the patches directory
        let mut entries = tokio::fs::read_dir(&patches_cache_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file() && path.extension().map_or(false, |ext| ext == "pwr") {
                tokio::fs::remove_file(&path).await?;
            }
        }

        progress_callback("cleanup", 100.0, "Patch cache cleaned");
    }

    Ok(())
}

/// Prepares the server directory for patching.
/// - Ensures HytaleServer.original exists (backing it up from HytaleServer.jar if needed).
/// - Deletes old patched JARs (HytaleServer.*.jar) except the original.
/// - Returns (source_jar, target_jar, recovered_source)
/// Restores the original vanilla JAR if a .original backup exists.
/// This prevents using a contaminated/patched JAR with the new agent system.
pub fn ensure_vanilla_jar(server_dir: &std::path::Path) -> Result<()> {
    let jar_path = server_dir.join("HytaleServer.jar");
    let original_path = server_dir.join("HytaleServer.original");

    // 1. If .original exists, it's our source of truth
    if original_path.exists() {
        println!("[Patcher] Restoring HytaleServer.jar from .original backup...");

        // Remove potentially contaminated jar
        if jar_path.exists() {
            let _ = std::fs::remove_file(&jar_path);
        }

        std::fs::copy(&original_path, &jar_path)?;
    } else if jar_path.exists() {
        // If no original exists but we have a jar, create the backup now
        // This should happen on first run with the new launcher
        println!("[Patcher] Creating vanilla backup HytaleServer.original");
        std::fs::copy(&jar_path, &original_path)?;
    }

    // 2. Cleanup: Remove any old HytaleServer.*.jar parches
    if let Ok(entries) = std::fs::read_dir(server_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                let name_low = name.to_lowercase();
                if name_low.starts_with("hytaleserver.")
                    && name_low.ends_with(".jar")
                    && name_low != "hytaleserver.jar"
                    && name_low != "hytaleserver.original"
                {
                    println!("[Patcher] Removing old/conflicting JAR: {}", name);
                    let _ = std::fs::remove_file(path);
                }
            }
        }
    }

    Ok(())
}

pub fn setup_java_proxy(java_real: &PathBuf) -> anyhow::Result<PathBuf> {
    let bin_dir = java_real
        .parent()
        .ok_or_else(|| anyhow::anyhow!("No parent dir"))?;
    let exe_name = if cfg!(windows) { "java.exe" } else { "java" };
    let original_name = if cfg!(windows) {
        "java_original.exe"
    } else {
        "java_original"
    };

    let java_proxy = bin_dir.join(exe_name);
    let java_original = bin_dir.join(original_name);

    if !java_original.exists() {
        std::fs::rename(java_real, &java_original)?;
    }

    // Always overwrite the proxy binary to ensure it matches the current launcher version
    let current_exe = std::env::current_exe()?;
    if let Err(e) = std::fs::copy(&current_exe, &java_proxy) {
        // If we can't copy (e.g. file busy), and it exists, we might warn but proceed.
        // However, for development/updates, this is critical.
        eprintln!(
            "[Patcher] Warning: Failed to update java proxy binary: {}",
            e
        );
        if !java_proxy.exists() {
            return Err(e.into());
        }
    }

    Ok(java_proxy)
}

pub fn remove_java_proxy(java_real: &PathBuf) -> anyhow::Result<()> {
    let bin_dir = java_real
        .parent()
        .ok_or_else(|| anyhow::anyhow!("No parent dir"))?;
    let exe_name = if cfg!(windows) { "java.exe" } else { "java" };
    let original_name = if cfg!(windows) {
        "java_original.exe"
    } else {
        "java_original"
    };

    let java_proxy = bin_dir.join(exe_name);
    let java_original = bin_dir.join(original_name);

    if java_original.exists() {
        if java_proxy.exists() {
            let _ = std::fs::remove_file(&java_proxy);
        }
        std::fs::rename(&java_original, &java_proxy)?;
    }
    Ok(())
}
