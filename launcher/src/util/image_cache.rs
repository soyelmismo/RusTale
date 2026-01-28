use anyhow::{Context, Result};
use iced::widget::image;
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::fs;

/// Intenta cargar la imagen de forma SINCRONA (bloqueante) si existe en cache.
/// Ideal para el background en el arranque, evitando el retraso de 1 segundo.
/// Retorna `Some(Handle)` si la imagen esta en cache, `None` si no existe.
pub fn load_image_sync_if_exists(url: &str) -> Option<image::Handle> {
    // 1. Calcular hash sincronamente
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    let hash = hex::encode(hasher.finalize());

    // 2. Resolver ruta de cache (duplicamos logica para evitar async)
    let base_dir = crate::config::get_app_dir();
    let file_path = base_dir
        .join("cache")
        .join("images")
        .join(format!("{}.jpg", hash));

    // 3. Si existe, leer inmediatamente con std::fs (bloqueante pero rapido)
    if file_path.exists() {
        if let Ok(data) = std::fs::read(&file_path) {
            return Some(image::Handle::from_bytes(data));
        }
    }
    None
}

/// Carga una imagen desde cache o descarga y guarda en cache usando el cliente compartido
pub async fn load_image(client: &Client, url: &str) -> Result<image::Handle> {
    let file_path = get_image_path(client, url, "images").await?;

    // Lectura ASINCRONA (no congela la UI)
    let data = fs::read(&file_path)
        .await
        .context("Failed to read image file")?;

    Ok(image::Handle::from_bytes(data))
}

/// Carga una imagen de noticia especificamente con formato correcto
pub async fn load_news_image(client: &Client, s3_key: &str) -> Result<image::Handle> {
    // URL especifica para imagenes de noticias de Hytale
    let url = format!("https://cdn.hytale.com/variants/blog_thumb_{}", s3_key);
    load_image(client, &url).await
}

/// Obtiene la ruta de la imagen en cache (descarga si no existe)
async fn get_image_path(client: &Client, url: &str, cache_subdir: &str) -> Result<PathBuf> {
    let cache_dir = crate::config::get_cache_dir(cache_subdir).await;

    // Crear directorio asincronamente
    if !cache_dir.exists() {
        fs::create_dir_all(&cache_dir).await?;
    }

    // El hashing es CPU intensive, lo movemos a un thread pool para no laggear la UI
    let url_string = url.to_string();
    let hash = tokio::task::spawn_blocking(move || {
        let mut hasher = Sha256::new();
        hasher.update(url_string.as_bytes());
        hex::encode(hasher.finalize())
    })
    .await
    .context("Hashing task failed")?;

    let file_path = cache_dir.join(format!("{}.jpg", hash));

    if !file_path.exists() {
        download_and_save_image(client, url, &file_path).await?;
    }

    Ok(file_path)
}

/// Descarga una imagen y la guarda en la ruta especificada
async fn download_and_save_image(client: &Client, url: &str, file_path: &Path) -> Result<()> {
    // Usamos el cliente compartido, ya no creamos uno nuevo aqui.
    let response = client.get(url).send().await.context("Request failed")?;

    if !response.status().is_success() {
        anyhow::bail!("HTTP error: {}", response.status());
    }

    let bytes = response.bytes().await.context("Failed to read response")?;

    // Escritura ASINCRONA
    fs::write(file_path, &bytes)
        .await
        .context("Failed to save image")?;
    Ok(())
}
