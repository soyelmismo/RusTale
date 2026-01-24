pub mod icons;
pub mod image_cache;

pub fn open_game_folder() {
    let path = crate::config::get_app_dir();
    open_path(path);
}

pub fn open_path(path: std::path::PathBuf) {
    // 1. Asegurar que la carpeta existe antes de intentar abrirla o normalizarla
    if !path.exists() {
        let _ = std::fs::create_dir_all(&path);
    }

    // 2. Intentar obtener la ruta "canónica" (absoluta y real)
    let final_path = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => path, // Si falla, usamos la original
    };

    println!("Abriendo carpeta: {:?}", final_path);

    if let Err(e) = open::that(final_path) {
        eprintln!("Error opening folder: {}", e);
    }
}

/// Sets execution permissions (755) on Unix systems.
/// Does nothing on other platforms.
pub async fn make_executable(path: &std::path::PathBuf) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if path.exists() {
            let meta = tokio::fs::metadata(path).await?;
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            tokio::fs::set_permissions(path, perms).await?;
        }
    }
    let _ = path; // avoid unused variable on non-unix
    Ok(())
}

/// Busca un puerto libre aleatorio entre 10000 y 65535
pub fn find_free_port() -> u16 {
    use rand::Rng;
    let mut rng = rand::rng();

    // Intentamos hasta 100 veces encontrar un puerto libre
    for _ in 0..100 {
        let port = rng.random_range(10000..=65535);
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }

    // Fallback: Si falla el aleatorio, retornar el default antiguo por seguridad.
    59313
}
