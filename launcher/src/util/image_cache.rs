use anyhow::{Context, Result};
use iced::widget::image;
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const STREAM_BUFFER_SIZE: usize = 64 * 1024; // 64KB chunks para streaming eficiente

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

    // 3. Si existe, leer con streaming optimizado para reducir picos de memoria
    if file_path.exists() {
        match read_image_streaming_sync(&file_path) {
            Ok(data) => return Some(image::Handle::from_bytes(data)),
            Err(_) => {
                // Si falla el streaming, intentar método tradicional como fallback
                if let Ok(data) = std::fs::read(&file_path) {
                    return Some(image::Handle::from_bytes(data));
                }
            }
        }
    }
    None
}

/// Carga una imagen desde cache o descarga y guarda en cache usando el cliente compartido
pub async fn load_image(client: &Client, url: &str) -> Result<image::Handle> {
    let file_path = get_image_path(client, url, "images").await?;

    // Lectura ASINCRONA con streaming para reducir picos de memoria
    let data = read_image_streaming(&file_path)
        .await
        .context("Failed to read image file with streaming")?;

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

    // Hashing directo en el thread actual (es muy rapido para strings cortos)
    let hash = {
        let mut hasher = Sha256::new();
        hasher.update(url.as_bytes());
        hex::encode(hasher.finalize())
    };

    let file_path = cache_dir.join(format!("{}.jpg", hash));

    if !file_path.exists() {
        download_and_save_image(client, url, &file_path).await?;
    }

    Ok(file_path)
}

/// Descarga una imagen y la guarda en la ruta especificada usando streaming
async fn download_and_save_image(client: &Client, url: &str, file_path: &Path) -> Result<()> {
    // Usamos el cliente compartido, ya no creamos uno nuevo aqui.
    let response = client.get(url).send().await.context("Request failed")?;

    if !response.status().is_success() {
        anyhow::bail!("HTTP error: {}", response.status());
    }

    // Crear el archivo de destino
    let mut file = tokio::fs::File::create(file_path).await
        .context("Failed to create destination file")?;

    // Obtener el stream de bytes del response
    let mut stream = response.bytes_stream();
    
    // Buffer para procesar chunks
    let mut buffer = Vec::with_capacity(STREAM_BUFFER_SIZE);
    
    use futures::StreamExt;
    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.context("Failed to read chunk from response")?;
        buffer.extend_from_slice(&chunk);
        
        // Si el buffer alcanza el tamaño límite, escribirlo en disco
        if buffer.len() >= STREAM_BUFFER_SIZE {
            file.write_all(&buffer).await
                .context("Failed to write chunk to file")?;
            buffer.clear();
        }
    }
    
    // Escribir cualquier remaining data
    if !buffer.is_empty() {
        file.write_all(&buffer).await
            .context("Failed to write final chunk to file")?;
    }
    
    file.flush().await.context("Failed to flush file")?;
    
    Ok(())
}

/// Lee una imagen usando streaming asíncrono para reducir picos de memoria
async fn read_image_streaming(file_path: &Path) -> Result<Vec<u8>> {
    let mut file = tokio::fs::File::open(file_path).await
        .context("Failed to open image file")?;
    
    let mut buffer = Vec::with_capacity(STREAM_BUFFER_SIZE);
    let mut temp_buffer = [0u8; STREAM_BUFFER_SIZE];
    
    loop {
        let bytes_read = file.read(&mut temp_buffer).await
            .context("Failed to read from image file")?;
        
        if bytes_read == 0 {
            break; // EOF
        }
        
        buffer.extend_from_slice(&temp_buffer[..bytes_read]);
    }
    
    Ok(buffer)
}

/// Lee una imagen usando streaming síncrono para reducir picos de memoria (fallback)
fn read_image_streaming_sync(file_path: &Path) -> Result<Vec<u8>> {
    use std::io::Read;
    
    let mut file = std::fs::File::open(file_path)
        .context("Failed to open image file")?;
    
    let mut buffer = Vec::with_capacity(STREAM_BUFFER_SIZE);
    let mut temp_buffer = [0u8; STREAM_BUFFER_SIZE];
    
    loop {
        let bytes_read = file.read(&mut temp_buffer)
            .context("Failed to read from image file")?;
        
        if bytes_read == 0 {
            break; // EOF
        }
        
        buffer.extend_from_slice(&temp_buffer[..bytes_read]);
    }
    
    Ok(buffer)
}
