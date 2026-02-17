use std::sync::Arc;

/// Asegura que Java este disponible usando la logica existente del launcher
pub async fn ensure_java_available(base_dir: &std::path::Path) -> anyhow::Result<JavaInfo> {
    // 1. Verificar si JRE ya esta instalado usando la logica existente
    let tools_dir = base_dir.join("tools");
    let jre_base_dir = tools_dir.join("jre");
    let latest_dir = jre_base_dir.join("latest");

    println!("[JRE Debug] base_dir: {}", base_dir.display());
    println!("[JRE Debug] latest_dir: {}", latest_dir.display());

    if crate::java::is_jre_installed_at(&latest_dir) {
        println!("[JRE] JRE already installed at: {}", latest_dir.display());

        // Movemos la operacion bloqueante a un hilo separado para no congelar la UI/LSD
        let version =
            tokio::task::spawn_blocking(move || get_java_version_sync(&latest_dir)).await??;
        // -------------------

        return Ok(JavaInfo { version });
    }

    // 2. Si no esta disponible, descargar usando el nuevo patch API
    let client = reqwest::Client::new();
    crate::game::patch_api::PatchApiFrontend::get_instance()
        .download_jre(
            &client,
            &base_dir.to_path_buf(),
            Arc::new(|component, progress, status, _total, _downloaded, _eta, _step| {
                eprintln!("Java {}: {:.1}% - {}", component, progress, status);
            }),
            None, // Sin token de cancelacion por ahora
        )
        .await?;

    // Clonamos latest_dir antes de moverlo al closure
    let latest_dir_clone = latest_dir.clone();
    let version =
        tokio::task::spawn_blocking(move || get_java_version_sync(&latest_dir_clone)).await??;
    // ---------------------------

    Ok(JavaInfo { version })
}

/// Obtiene version de Java de forma sincrona (para ejecutar en blocking task)
fn get_java_version_sync(jre_dir: &std::path::Path) -> anyhow::Result<String> {
    let java_bin = if cfg!(windows) {
        jre_dir.join("bin").join("java.exe")
    } else {
        jre_dir.join("bin").join("java")
    };

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
    if let Some(line) = stderr.lines().next() {
        if let Some(start) = line.find('"') {
            if let Some(end) = line[start + 1..].find('"') {
                return Ok(line[start + 1..start + 1 + end].to_string());
            }
        }
    }

    Ok("Unknown".to_string())
}

#[derive(Debug, Clone)]
pub struct JavaInfo {
    pub version: String,
}
