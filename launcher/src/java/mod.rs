use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::atomic;


pub mod proxy;



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

/// Checks if JRE is properly installed at the specified path
pub fn is_jre_installed_at(jre_path: &PathBuf) -> bool {
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
