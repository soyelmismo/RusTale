use anyhow::Result;
use std::path::PathBuf;

pub fn setup_java_proxy(java_real: &PathBuf) -> Result<PathBuf> {
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
            "[JavaProxy] Warning: Failed to update java proxy binary: {}",
            e
        );
        if !java_proxy.exists() {
            return Err(e.into());
        }
    }

    Ok(java_proxy)
}

pub fn remove_java_proxy(java_real: &PathBuf) -> Result<()> {
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
