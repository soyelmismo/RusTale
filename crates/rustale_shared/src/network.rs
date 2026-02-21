use anyhow::{Result, anyhow, bail};
use futures::StreamExt;
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::fs::{OpenOptions, File};
use tokio::io::AsyncWriteExt;
use once_cell::sync::Lazy;
use reqwest::Client;

#[cfg(feature = "security")]
pub static SECURE_HTTP_CLIENT: Lazy<rustale_security::SecureClient> = Lazy::new(|| {
    rustale_security::SecureClient::builder()
        .with_pinning(rustale_security::get_pinned_cert_hash)
        .build()
});

/// Singleton-like HTTP Client
pub static HTTP_CLIENT: Lazy<Client> = Lazy::new(|| {
    // Instalar el proveedor criptográfico de Rustls a nivel de proceso
    // Ignoramos el error (Result::ok) por si otro hilo o crate ya lo instaló
    rustls::crypto::ring::default_provider().install_default().ok();

    Client::builder()
        .user_agent(format!("RusTale/{}", env!("CARGO_PKG_VERSION")))
        .pool_idle_timeout(std::time::Duration::from_secs(30))
        .tcp_keepalive(std::time::Duration::from_secs(60))
        .build()
        .expect("Failed to create unified HTTP client")
});

/// Format bytes in human-readable format
pub fn format_bytes(bytes: u64) -> String {
    if bytes > 1_073_741_824 {
        format!("{:.2} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes > 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes > 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

/// Format speed in human-readable format
pub fn format_speed(bytes_per_sec: f64) -> String {
    if bytes_per_sec > 1_048_576.0 {
        format!("{:.2} MB/s", bytes_per_sec / 1_048_576.0)
    } else if bytes_per_sec > 1024.0 {
        format!("{:.2} KB/s", bytes_per_sec / 1024.0)
    } else {
        format!("{:.0} B/s", bytes_per_sec)
    }
}

const MAX_RETRIES: u32 = 10;

/// Centralized downloader with progress support and resumption
pub async fn download_file<F>(
    url: &str,
    destination: &Path,
    progress_callback: F,
    cancel_token: Option<Arc<AtomicBool>>,
) -> Result<()> 
where 
    F: Fn(String, f64, String, u64, u64, Option<String>, Option<usize>)
{
    let temp_destination = destination.with_extension("downloading");

    if let Some(parent) = destination.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    #[cfg(feature = "security")]
    let z_base = rustale_security::get_private_var("Z_A");
    #[cfg(feature = "security")]
    let is_z = url.starts_with(&*z_base);
    #[cfg(not(feature = "security"))]
    let is_z = false;

    let mut total_size = 0u64;
    let _download_start_time = std::time::Instant::now();

    // Try to get total size via HEAD
    let head_req = if is_z {
        #[cfg(feature = "security")]
        {
            SECURE_HTTP_CLIENT.head(url)
                .header(&*rustale_security::get_private_var("Z_B"), &*rustale_security::get_private_var("Z_C"))
                .header(&*rustale_security::get_private_var("Z_E"), &*rustale_security::get_private_var("Z_D"))
                .header(&*rustale_security::get_private_var("Z_G"), &*rustale_security::get_private_var("Z_F"))
        }
        #[cfg(not(feature = "security"))]
        unreachable!()
    } else {
        HTTP_CLIENT.head(url)
    };

    if let Ok(resp) = head_req.send().await {
        if resp.status().is_success() {
            total_size = resp.content_length().unwrap_or(0);
        }
    }

    let mut attempt = 0;

    loop {
        if let Some(token) = &cancel_token {
            if token.load(Ordering::Relaxed) {
                return Err(anyhow!("Cancelled by user"));
            }
        }

        attempt += 1;

        let mut downloaded_len = 0u64;
        if temp_destination.exists() {
            if let Ok(metadata) = tokio::fs::metadata(&temp_destination).await {
                downloaded_len = metadata.len();
            }
        }

        if total_size > 0 && downloaded_len == total_size {
            break;
        }

        let mut request_builder = if is_z {
            #[cfg(feature = "security")]
            {
                SECURE_HTTP_CLIENT.get(url)
                    .header(&*rustale_security::get_private_var("Z_B"), &*rustale_security::get_private_var("Z_C"))
                    .header(&*rustale_security::get_private_var("Z_E"), &*rustale_security::get_private_var("Z_D"))
                    .header(&*rustale_security::get_private_var("Z_G"), &*rustale_security::get_private_var("Z_F"))
            }
            #[cfg(not(feature = "security"))]
            unreachable!()
        } else {
            HTTP_CLIENT.get(url)
        };
        if downloaded_len > 0 {
            request_builder = request_builder.header("Range", format!("bytes={}-", downloaded_len));
        }

        let response_result = request_builder.send().await;

        match response_result {
            Ok(response) => {
                let status = response.status();

                if !status.is_success() {
                    if status.is_client_error() {
                        bail!("Download failed with status: {} for URL: {}", status, url);
                    }
                } else {
                    if total_size == 0 {
                        if let Some(content_len) = response.content_length() {
                            if status == reqwest::StatusCode::PARTIAL_CONTENT {
                                total_size = downloaded_len + content_len;
                            } else {
                                total_size = content_len;
                            }
                        }
                    }
                    
                    let mut file = if status == reqwest::StatusCode::PARTIAL_CONTENT {
                        OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&temp_destination)
                            .await?
                    } else {
                        downloaded_len = 0;
                        File::create(&temp_destination).await?
                    };

                    let mut stream = response.bytes_stream();
                    let mut last_report = std::time::Instant::now();
                    let mut bytes_since_last_report: u64 = 0;
                    let mut stream_failed = false;

                    while let Some(chunk_result) = stream.next().await {
                        if let Some(token) = &cancel_token {
                            if token.load(Ordering::Relaxed) {
                                let _ = file.flush().await;
                                return Err(anyhow!("Cancelled by user"));
                            }
                        }

                        match chunk_result {
                            Ok(chunk) => {
                                if let Err(_) = file.write_all(&chunk).await {
                                    stream_failed = true;
                                    break;
                                }
                                let len = chunk.len() as u64;
                                downloaded_len += len;
                                bytes_since_last_report += len;

                                if last_report.elapsed().as_millis() > 200 {
                                    let speed = bytes_since_last_report as f64
                                        / last_report.elapsed().as_secs_f64();
                                    let speed_str = format_speed(speed);

                                    let pct = if total_size > 0 {
                                        (downloaded_len as f64 / total_size as f64) * 100.0
                                    } else {
                                        0.0
                                    };

                                    let eta = if total_size > 0 && speed > 0.0 {
                                        let remaining_bytes = total_size.saturating_sub(downloaded_len) as f64;
                                        let seconds_remaining = remaining_bytes / speed;
                                        if seconds_remaining < 60.0 {
                                            Some(format!("{:.0}s", seconds_remaining))
                                        } else if seconds_remaining < 3600.0 {
                                            Some(format!("{:.0}m", seconds_remaining / 60.0))
                                        } else {
                                            Some(format!("{:.1}h", seconds_remaining / 3600.0))
                                        }
                                    } else {
                                        None
                                    };

                                    progress_callback(
                                        "download".to_string(),
                                        pct,
                                        speed_str,
                                        total_size,
                                        downloaded_len,
                                        eta,
                                        None,
                                    );
                                    last_report = std::time::Instant::now();
                                    bytes_since_last_report = 0;
                                }
                            }
                            Err(_) => {
                                stream_failed = true;
                                break;
                            }
                        }
                    }

                    if !stream_failed {
                        file.flush().await?;
                        if total_size > 0 && downloaded_len == total_size {
                            break;
                        } else if total_size == 0 && downloaded_len > 0 {
                            break;
                        }
                    }
                }
            }
            Err(_) => {}
        }

        if attempt >= MAX_RETRIES {
            bail!("Failed to download after {} attempts: {}", MAX_RETRIES, url);
        }

        let wait = std::time::Duration::from_secs(2u64.pow(attempt.min(4) as u32));
        tokio::time::sleep(wait).await;
    }

    tokio::fs::rename(&temp_destination, destination).await?;
    progress_callback("download".to_string(), 100.0, "Complete".to_string(), total_size, total_size, None, None);
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // === Essential Tests Only ===
    // Formatting functions are cosmetic - only test boundary conditions

    #[test]
    fn test_format_bytes_boundaries() {
        // Test the key boundary: when unit changes
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1025), "1.0 KB");  // first KB value
        assert_eq!(format_bytes(1_048_577), "1.0 MB");  // first MB value
        assert_eq!(format_bytes(1_073_741_825), "1.00 GB");  // first GB value
    }

    #[test]
    fn test_format_speed_boundaries() {
        assert_eq!(format_speed(0.0), "0 B/s");
        assert_eq!(format_speed(1025.0), "1.00 KB/s");
        assert_eq!(format_speed(1_048_577.0), "1.00 MB/s");
    }

    #[test]
    fn test_http_client_exists() {
        // Verify the client is initialized
        let _ = &*HTTP_CLIENT;
    }
}
