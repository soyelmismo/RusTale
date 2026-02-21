use std::sync::Arc;
use rustale_shared::java::detection::{JavaInfo, get_java_version_sync};

/// Asegura que Java este disponible usando la logica existente del launcher
pub async fn ensure_java_available(base_dir: &std::path::Path) -> anyhow::Result<JavaInfo> {
    // 1. Verificar si JRE ya esta instalado usando la logica existente
    let tools_dir = base_dir.join("tools");
    let jre_base_dir = tools_dir.join("jre");
    let latest_dir = jre_base_dir.join("latest");

    println!("[JRE Debug] base_dir: {}", base_dir.display());
    println!("[JRE Debug] latest_dir: {}", latest_dir.display());

    if rustale_shared::java::is_jre_installed_at(&latest_dir) {
        println!("[JRE] JRE already installed at: {}", latest_dir.display());

        // Movemos la operacion bloqueante a un hilo separado para no congelar la UI/LSD
        let version =
            tokio::task::spawn_blocking(move || get_java_version_sync(&latest_dir)).await??;
        // -------------------

        return Ok(JavaInfo { version });
    }

    // 2. Si no esta disponible, descargar usando el nuevo patch API
    crate::game::patch_api::PatchApiFrontend::get_instance()
        .download_jre(
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
