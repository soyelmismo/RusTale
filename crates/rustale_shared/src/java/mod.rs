use anyhow::{Context, Result};
use std::path::PathBuf;

pub mod proxy;
pub mod detection;
pub mod installer;
pub mod tracking;
#[cfg(windows)]
pub mod win_job;

pub use installer::JreInstaller;

pub use proxy::{setup_java_proxy, remove_java_proxy, find_free_port, save_active_port, get_saved_port, run_java_proxy_logic, get_runtime_port_file, is_running_as_java_proxy};

/// Gets the path to the Java executable from the tools/jre/latest directory
pub fn get_java_exec(base_dir: &PathBuf) -> Result<String> {
    let java_bin = get_jre_exec_path(base_dir);

    if !java_bin.exists() {
        anyhow::bail!("JRE executable not found at: {}", java_bin.display());
    }

    java_bin
        .to_str()
        .map(|s| s.to_string())
        .context("Invalid Java path encoding")
}

/// Helper to get the platform-specific JRE executable path
pub fn get_jre_exec_path(base_dir: &PathBuf) -> PathBuf {
    let jre_path = base_dir.join("tools").join("jre").join("latest");
    if cfg!(windows) {
        jre_path.join("bin").join("java.exe")
    } else {
        jre_path.join("bin").join("java")
    }
}

/// Checks if JRE is properly installed at the specified path
pub fn is_jre_installed_at(jre_path: &std::path::Path) -> bool {
    if !jre_path.exists() {
        return false;
    }

    // Check for Java executable
    let java_exec = if cfg!(windows) {
        jre_path.join("bin").join("java.exe")
    } else {
        jre_path.join("bin").join("java")
    };

    java_exec.exists()
}
