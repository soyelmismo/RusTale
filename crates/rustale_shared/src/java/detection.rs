use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JavaInfo {
    pub version: String,
}

/// Ensures Java is available and returns version info
pub async fn ensure_java_available(base_dir: &std::path::Path) -> anyhow::Result<JavaInfo> {
    let jre_dir = crate::paths::GamePaths::new(base_dir.to_path_buf()).jre();
    
    if !jre_dir.exists() {
        anyhow::bail!("JRE not installed at: {}", jre_dir.display());
    }

    let version = get_java_version_sync(&jre_dir)?;
    Ok(JavaInfo { version })
}

/// Obtiene version de Java de forma sincrona (para ejecutar en blocking task)
pub fn get_java_version_sync(jre_dir: &std::path::Path) -> anyhow::Result<String> {
    let java_bin = if cfg!(windows) {
        jre_dir.join("bin").join("java.exe")
    } else {
        jre_dir.join("bin").join("java")
    };

    if !java_bin.exists() {
        anyhow::bail!("Java binary not found at: {}", java_bin.display());
    }

    // Configuracion para que no abra ventana de consola en Windows
    let mut cmd = std::process::Command::new(&java_bin);
    cmd.arg("-version");

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = cmd.output()?;

    if !output.status.success() {
        anyhow::bail!("Failed to execute java -version");
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    
    // Look for the line containing the actual Java version
    // The version line typically starts with "openjdk version" or contains version in quotes
    for line in stderr.lines() {
        if line.contains("openjdk version") || line.contains("java version") {
            // Extract version from quotes
            if let Some(start) = line.find('"') {
                if let Some(end) = line[start + 1..].find('"') {
                    return Ok(line[start + 1..start + 1 + end].to_string());
                }
            }
        }
    }

    Ok("Unknown".to_string())
}
