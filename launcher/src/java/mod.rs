use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};

#[derive(Debug, serde::Deserialize)]
struct JREPlatform {
    url: String,
    sha256: String,
}

#[derive(Debug, serde::Deserialize)]
struct JREJSON {
    version: String,
    download_url: std::collections::HashMap<String, std::collections::HashMap<String, JREPlatform>>,
}

/// Downloads and installs JRE if not already installed.
/// Installs into `.../RusTale/tools/jre/latest` to persist across game deletions.
pub async fn download_jre(
    client: &reqwest::Client,
    base_dir: &PathBuf,
    progress_callback: impl Fn(&str, f64, &str),
    cancel_token: Option<Arc<AtomicBool>>,
) -> Result<()> {
    // Define the persistent tools directory
    let tools_dir = base_dir.join("tools");
    let jre_base_dir = tools_dir.join("jre");
    let latest_dir = jre_base_dir.join("latest");

    // Check if JRE is already installed and valid
    if is_jre_installed_at(&latest_dir) {
        let java_exec = latest_dir.join("bin").join("java");
        let _ = crate::util::make_executable(&java_exec).await;
        progress_callback("jre", 100.0, "JRE already installed");
        return Ok(());
    }

    progress_callback("jre", 0.0, "Fetching JRE metadata...");

    let resp = client
        .get("https://launcher.hytale.com/version/release/jre.json")
        .send()
        .await
        .context("Failed to fetch JRE info")?;

    let jre_data: JREJSON = resp.json().await.context("Failed to parse JRE info")?;

    let os_name = get_os_name();
    let arch = get_arch_name();

    let os_data = jre_data
        .download_url
        .get(os_name)
        .context(format!("No JRE available for OS: {}", os_name))?;

    let platform = os_data.get(arch).context(format!(
        "No JRE available for arch: {} on {}",
        arch, os_name
    ))?;

    // Use a cache directory for the zip file
    let cache_dir = crate::config::get_cache_dir("jre").await;
    tokio::fs::create_dir_all(&jre_base_dir).await?;

    let file_name = platform.url.split('/').last().unwrap_or("jre.zip");
    let cache_file = cache_dir.join(file_name);

    // Download if not cached
    if !cache_file.exists() {
        progress_callback("jre", 10.0, &format!("Downloading {}...", file_name));

        crate::game::downloader::download_file(
            client,
            &platform.url,
            &cache_file,
            |pct, speed| {
                progress_callback(
                    "jre",
                    pct as f64,
                    &format!("Downloading {}... ({})", file_name, speed),
                );
            },
            cancel_token,
        )
        .await?;
    }

    progress_callback("jre", 70.0, "Verifying JRE integrity...");

    let cache_file_clone = cache_file.clone();
    let expected_sha = platform.sha256.clone();

    // Verify SHA256 in a blocking task
    tokio::task::spawn_blocking(move || verify_sha256(&cache_file_clone, &expected_sha))
        .await
        .context("SHA task join error")??;

    progress_callback("jre", 80.0, "Extracting JRE...");

    // Extract to a temporary folder first
    let temp_dir = jre_base_dir.join(format!("tmp-{}", &jre_data.version));
    if temp_dir.exists() {
        tokio::fs::remove_dir_all(&temp_dir).await?;
    }

    let cache_file_clone = cache_file.clone();
    let temp_dir_clone = temp_dir.clone();

    // Extract in a blocking task
    tokio::task::spawn_blocking(move || {
        extract_archive(&cache_file_clone, &temp_dir_clone)?;
        flatten_jre_dir(&temp_dir_clone)
    })
    .await
    .context("Extraction task join error")??;

    progress_callback("jre", 95.0, "Finalizing installation...");

    // Remove old version if exists
    if latest_dir.exists() {
        tokio::fs::remove_dir_all(&latest_dir).await?;
    }

    // Atomic rename (or move)
    tokio::fs::rename(&temp_dir, &latest_dir)
        .await
        .context("Failed to move JRE to final location")?;

    // Set executable permissions on Unix
    let java_exec = latest_dir.join("bin").join("java");
    let _ = crate::util::make_executable(&java_exec).await;

    // --- NUEVO: Limpieza del ZIP ---
    // Como ya instalamos, borramos el ZIP para ahorrar espacio y evitar reusar uno corrupto en el futuro
    if cache_file.exists() {
        let _ = tokio::fs::remove_file(&cache_file).await;
    }
    // -------------------------------

    progress_callback("jre", 100.0, "JRE ready");

    Ok(())
}

/// Gets the path to the Java executable from the tools/jre/latest directory
pub fn get_java_exec(base_dir: &PathBuf) -> Result<String> {
    let paths = crate::game::paths::GamePaths::new(base_dir.clone());
    let java_bin = paths.java_exec();

    if !java_bin.exists() {
        anyhow::bail!("JRE executable not found at: {}", java_bin.display());
    }

    java_bin
        .to_str()
        .map(|s| s.to_string())
        .context("Invalid Java path encoding")
}

pub fn is_jre_installed_at(jre_dir: &PathBuf) -> bool {
    // We already have the dir, but we can't easily use GamePaths here
    // without potentially changing the logic (is_jre_installed_at is called with latest_dir).
    // However, if we want to use the unified paths:
    let java_bin = if cfg!(windows) {
        jre_dir.join("bin").join("java.exe")
    } else {
        jre_dir.join("bin").join("java")
    };
    java_bin.exists()
}

fn get_os_name() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "darwin",
        "linux" => "linux",
        other => other,
    }
}

fn get_arch_name() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    }
}

fn verify_sha256(path: &PathBuf, expected: &str) -> Result<()> {
    use sha2::{Digest, Sha256};

    let data = std::fs::read(path).context("Failed to read file for verification")?;
    let mut hasher = Sha256::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let actual = hex::encode(result);

    if actual.to_lowercase() != expected.to_lowercase() {
        anyhow::bail!("SHA256 mismatch");
    }
    Ok(())
}

fn extract_archive(archive_path: &PathBuf, dest_dir: &PathBuf) -> Result<()> {
    std::fs::create_dir_all(dest_dir)?;

    let file = std::fs::File::open(archive_path).context("Failed to open archive")?;

    if archive_path.to_string_lossy().ends_with(".zip") {
        let mut archive = zip::ZipArchive::new(file).context("Failed to read ZIP archive")?;
        archive.extract(dest_dir).context("Failed to extract ZIP")?;
    } else if archive_path.to_string_lossy().ends_with(".tar.gz") {
        let gz = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(gz);
        archive
            .unpack(dest_dir)
            .context("Failed to extract tar.gz")?;
    } else {
        anyhow::bail!("Unsupported archive format");
    }
    Ok(())
}

fn flatten_jre_dir(jre_dir: &PathBuf) -> Result<()> {
    let entries: Vec<_> = std::fs::read_dir(jre_dir)?.filter_map(|e| e.ok()).collect();

    // If there is only one directory inside, move everything up
    if entries.len() != 1 {
        return Ok(());
    }

    let entry = &entries[0];
    if !entry.path().is_dir() {
        return Ok(());
    }

    let nested = entry.path();
    let files: Vec<_> = std::fs::read_dir(&nested)?.filter_map(|e| e.ok()).collect();

    for f in files {
        let old_path = f.path();
        let new_path = jre_dir.join(f.file_name());
        std::fs::rename(&old_path, &new_path)?;
    }

    std::fs::remove_dir_all(&nested)?;
    Ok(())
}
