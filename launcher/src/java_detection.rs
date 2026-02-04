/// Asegura que Java este disponible usando la logica existente del launcher
pub async fn ensure_java_available(base_dir: &std::path::Path) -> anyhow::Result<JavaInfo> {
    // 1. Verificar si JRE ya esta instalado usando la logica existente
    let tools_dir = base_dir.join("tools");
    let jre_base_dir = tools_dir.join("jre");
    let latest_dir = jre_base_dir.join("latest");

    if crate::java::is_jre_installed_at(&latest_dir) {
        // Java ya esta disponible
        let java_exec = crate::java::get_java_exec(&base_dir.to_path_buf())?;

        // Movemos la operacion bloqueante a un hilo separado para no congelar la UI/LSD
        let version =
            tokio::task::spawn_blocking(move || get_java_version_sync(&latest_dir)).await??;
        // -------------------

        return Ok(JavaInfo {
            path: java_exec,
            version,
            source: JavaSource::Managed,
        });
    }

    // 2. Si no esta disponible, descargar usando la logica existente
    let client = reqwest::Client::new();
    crate::java::download_jre(
        &client,
        &base_dir.to_path_buf(),
        |component, progress, status| {
            eprintln!("Java {}: {:.1}% - {}", component, progress, status);
        },
        None, // Sin token de cancelacion por ahora
    )
    .await?;

    // 3. Verificar que se instalo correctamente
    let java_exec = crate::java::get_java_exec(&base_dir.to_path_buf())?;

    // Clonamos latest_dir antes de moverlo al closure
    let latest_dir_clone = latest_dir.clone();
    let version =
        tokio::task::spawn_blocking(move || get_java_version_sync(&latest_dir_clone)).await??;
    // ---------------------------

    Ok(JavaInfo {
        path: java_exec,
        version,
        source: JavaSource::Managed,
    })
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
    pub path: String,
    pub version: String,
    pub source: JavaSource,
}

#[derive(Debug, Clone)]
pub enum JavaSource {
    Managed,
}
