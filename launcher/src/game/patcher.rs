use anyhow::{Context, Result};
use std::io::{BufReader, BufWriter, Read, Write};
use std::sync::{Arc, atomic::AtomicBool};
use std::{path::PathBuf, process::Stdio};
use tokio::io::AsyncBufReadExt;
use zip::write::SimpleFileOptions;

use crate::config::OnlineFixMode;

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
    let latest = find_latest_version(client, channel).await?;

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

    // Download using downloader utility with progress
    crate::game::downloader::download_file(
        client,
        url,
        &zip_path,
        |pct, speed| {
            progress_callback(
                "butler",
                pct as f64,
                &format!("Downloading Butler... ({})", speed),
            );
        },
        cancel_token,
    )
    .await?;

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
pub async fn find_latest_version(client: &reqwest::Client, channel: &str) -> Result<i32> {
    let os_name = std::env::consts::OS;
    let arch = get_arch_name();

    // Try known versions first
    let known_versions = vec![100, 50, 25, 10, 5, 1];
    let mut found_base = 0;

    println!("Searching for base version...");
    for version in known_versions {
        let url = format!(
            "https://game-patches.hytale.com/patches/{}/{}/{}/0/{}.pwr",
            os_name, arch, channel, version
        );

        if let Ok(resp) = client.head(&url).send().await {
            if resp.status().is_success() {
                found_base = version;
                println!("Found base version {}", version);
                break;
            }
        }

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    if found_base == 0 {
        anyhow::bail!(
            "Cannot reach game server or no versions available for {}/{}",
            os_name,
            arch
        );
    }

    // Linear search for latest version
    let mut latest = found_base;
    let max_check = found_base + 50;

    println!("Searching for latest version...");
    for version in (found_base + 1)..=max_check.min(200) {
        let url = format!(
            "https://game-patches.hytale.com/patches/{}/{}/{}/0/{}.pwr",
            os_name, arch, channel, version
        );

        if let Ok(resp) = client.head(&url).send().await {
            if resp.status().is_success() {
                latest = version;
                println!("Found version {}", version);
            } else {
                break;
            }
        } else {
            break;
        }

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    println!("Latest version found: {}", latest);
    Ok(latest)
}

/// Downloads a PWR patch file
pub async fn download_pwr(
    client: &reqwest::Client,
    channel: &str,
    prev_version: i32,
    target_version: i32,
    progress_callback: &impl Fn(&str, f64, &str),
    cancel_token: Option<Arc<AtomicBool>>,
) -> Result<PathBuf> {
    let cache_dir = crate::config::get_cache_dir("game_patches").await;

    let os_name = std::env::consts::OS;
    let arch = get_arch_name();

    let file_name = format!("{}-{}.pwr", prev_version, target_version);
    let dest = cache_dir.join(&file_name);

    // Check if already cached
    if dest.exists() {
        progress_callback("download", 40.0, "PWR file cached");
        return Ok(dest);
    }

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

    // Download file with progress
    crate::game::downloader::download_file(
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
    )
    .await?;

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

/// Parches the HytaleServer.jar to redirect authentication URLs to localhost/sanasol.
/// This is used by both the Runner (pre-patching) and the Proxy (JIT patching).
pub fn patch_server_jar(
    src: &PathBuf,
    dst: &PathBuf,
    online_mode: OnlineFixMode,
    port: u16,
    progress: Option<Box<dyn Fn(f32) + Send>>,
) -> anyhow::Result<()> {
    if !src.exists() {
        anyhow::bail!("Source server JAR not found at {:?}", src);
    }

    let file_in = std::fs::File::open(src)?;
    let reader = BufReader::new(file_in);
    let mut archive = zip::ZipArchive::new(reader)?;

    let file_out = std::fs::File::create(dst)?;
    let writer = BufWriter::new(file_out);
    let mut zip_writer = zip::ZipWriter::new(writer);

    // Replacements logic
    let replacements = if online_mode == OnlineFixMode::Sanasol {
        vec![
            (
                "https://sessions.hytale.com",
                "https://sessions.sanasol.ws".into(),
            ),
            ("https://api.hytale.com", "https://api.sanasol.ws".into()),
            (
                "https://account.hytale.com",
                "https://account.sanasol.ws".into(),
            ),
        ]
    } else {
        vec![
            (
                "https://sessions.hytale.com",
                format!("http://127.0.0.000001:{}", port),
            ),
            (
                "https://api.hytale.com",
                format!("http://127.0.0.1:{}", port),
            ),
            (
                "https://account.hytale.com",
                format!("http://127.0.0.00001:{}", port),
            ),
        ]
    };

    let mut byte_replacements = Vec::new();
    for (target, replacement) in replacements {
        let rep_bytes = replacement.into_bytes();
        // Padding with 'spaces' to match length
        //while rep_bytes.len() < target.len() {
        //    rep_bytes.push(b' ');
        //}
        byte_replacements.push((target.as_bytes().to_vec(), rep_bytes));
    }

    let mut buffer = Vec::new();
    let total_files = archive.len();

    for i in 0..total_files {
        if let Some(ref cb) = progress {
            cb((i as f32 / total_files as f32) * 100.0);
        }
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();

        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored) // Store for speed
            .unix_permissions(file.unix_mode().unwrap_or(0o755));

        buffer.clear();
        file.read_to_end(&mut buffer)?;

        // Apply patch only to code/config files
        if name.ends_with(".class") || name.ends_with(".json") || name.ends_with(".properties") {
            for (target, replacement) in &byte_replacements {
                replace_bytes(&mut buffer, target, replacement);
            }
        }

        zip_writer.start_file(&name, options)?;
        zip_writer.write_all(&buffer)?;
    }

    zip_writer.finish()?;

    if let Some(ref cb) = progress {
        cb(100.0);
    }

    Ok(())
}

fn replace_bytes(data: &mut [u8], target: &[u8], replacement: &[u8]) {
    let len = data.len();
    let pat_len = target.len();
    if len < pat_len {
        return;
    }

    for i in 0..=(len - pat_len) {
        if data[i] == target[0] && &data[i..i + pat_len] == target {
            for (j, &b) in replacement.iter().enumerate() {
                data[i + j] = b;
            }
        }
    }
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
        eprintln!("[Patcher] Warning: Failed to update java proxy binary: {}", e);
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


/// Generates the AOT Cache for a specific JAR.
/// Warning: This operation might time out or hang if the server doesn't exit automatically.
/// Ideally interact with the process or wait for a specific log line then kill it.
pub fn generate_server_aot(
    java_exec: &PathBuf,
    jar_path: &PathBuf,
    jvm_args: &str,
    app_args: &[String],
) -> anyhow::Result<()> {
    if !jar_path.exists() {
        return Err(anyhow::anyhow!("JAR file not found"));
    }

    let aot_config = jar_path.with_extension("aot_config");
    let aot_cache = jar_path.with_extension("aot");

    // --- PHASE 1: RECORD ---
    println!("[AOT] Phase 1: Recording configuration...");
    let mut cmd_record = std::process::Command::new(java_exec);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd_record.creation_flags(0x08000000);
    }

    cmd_record
        .arg("-XX:AOTMode=record")
        .arg(format!("-XX:AOTConfiguration={}", aot_config.to_string_lossy()))
        .arg("-Xlog:aot");

    for arg in jvm_args.split_whitespace() {
        if !arg.starts_with("-XX:AOT") {
            cmd_record.arg(arg);
        }
    }

    cmd_record.arg("-jar").arg(jar_path);
    for arg in app_args {
        cmd_record.arg(arg);
    }

    cmd_record.stdout(std::process::Stdio::piped());
    cmd_record.stderr(std::process::Stdio::inherit());

    let mut child = cmd_record.spawn()?;
    let stdout = child.stdout.take().unwrap();
    let reader = std::io::BufReader::new(stdout);

    use std::io::BufRead;
    for line in reader.lines() {
        if let Ok(l) = line {
            println!("[AOT-Record] {}", l);
            if l.contains("AOTConfiguration recorded") {
                println!("[AOT] Configuration ready. Stopping record process.");
                break;
            }
        }
    }

    let _ = child.kill();
    let _ = child.wait();

    if !aot_config.exists() {
        return Err(anyhow::anyhow!("Failed to generate AOT configuration"));
    }

    // --- PHASE 2: CREATE ---
    println!("[AOT] Phase 2: Creating cache archive...");
    let mut cmd_create = std::process::Command::new(java_exec);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd_create.creation_flags(0x08000000);
    }

    cmd_create
        .arg("-XX:AOTMode=create")
        .arg(format!("-XX:AOTConfiguration={}", aot_config.to_string_lossy()))
        .arg(format!("-XX:AOTCache={}", aot_cache.to_string_lossy()))
        .arg("-Xlog:aot");

    for arg in jvm_args.split_whitespace() {
        if !arg.starts_with("-XX:AOT") {
            cmd_create.arg(arg);
        }
    }

    cmd_create.arg("-jar").arg(jar_path);
    for arg in app_args {
        cmd_create.arg(arg);
    }

    cmd_create.stdout(std::process::Stdio::piped());
    cmd_create.stderr(std::process::Stdio::inherit());

    let mut child = cmd_create.spawn()?;
    let stdout = child.stdout.take().unwrap();
    let reader = std::io::BufReader::new(stdout);

    for line in reader.lines() {
        if let Ok(l) = line {
            println!("[AOT-Create] {}", l);
            if l.contains("AOTCache creation is complete") {
                println!("[AOT] Cache ready. Stopping creation process.");
                break;
            }
        }
    }

    let _ = child.kill();
    let _ = child.wait();

    if aot_cache.exists() {
        let _ = std::fs::remove_file(aot_config);
        println!("[AOT] Generation successful.");
        Ok(())
    } else {
        Err(anyhow::anyhow!("Failed to generate AOT cache"))
    }
}

/// Helper to swap AOT files similar to the JARs, to avoid crashes.
/// Returns true if it performed a swap (disabled original AOT).
pub fn handle_aot_backups(server_dir: &std::path::Path) -> anyhow::Result<bool> {
    let original_aot = server_dir.join("HytaleServer.aot");
    let backup_aot = server_dir.join("HytaleServer.aot.original");
    
    if original_aot.exists() && !backup_aot.exists() {
        println!("[Patcher] Backing up original AOT cache to avoid mismatch.");
        std::fs::rename(&original_aot, &backup_aot)?;
        return Ok(true);
    }
    Ok(false)
}
