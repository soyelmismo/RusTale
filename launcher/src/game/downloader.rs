use anyhow::Result;
use futures::StreamExt;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

const MAX_RETRIES: u32 = 10;

pub async fn download_file(
    client: &reqwest::Client,
    url: &str,
    destination: &PathBuf,
    progress_callback: impl Fn(f32, String),
    cancel_token: Option<Arc<AtomicBool>>,
) -> Result<()> {
    let temp_destination = destination.with_extension("downloading");

    if let Some(parent) = destination.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let mut total_size = 0u64;

    if let Ok(resp) = client.head(url).send().await {
        if resp.status().is_success() {
            total_size = resp.content_length().unwrap_or(0);
        }
    }

    let mut attempt = 0;

    loop {
        if let Some(token) = &cancel_token {
            if token.load(Ordering::Relaxed) {
                return Err(anyhow::anyhow!("Cancelled by user"));
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

        if attempt > 1 {
            println!(
                "Download attempt {}: Resuming from {} bytes. URL: {}",
                attempt, downloaded_len, url
            );
        }

        let mut request_builder = client.get(url);

        if downloaded_len > 0 {
            request_builder = request_builder.header("Range", format!("bytes={}-", downloaded_len));
        }

        let response_result = request_builder.send().await;

        match response_result {
            Ok(response) => {
                let status = response.status();

                if !status.is_success() {
                    if status.is_client_error() {
                        anyhow::bail!("Download failed with status: {} for URL: {}", status, url);
                    }
                    println!("Server error {}, retrying...", status);
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
                        tokio::fs::File::create(&temp_destination).await?
                    };

                    let mut stream = response.bytes_stream();
                    let mut last_report = std::time::Instant::now();
                    let mut bytes_since_last_report: u64 = 0;
                    let mut stream_failed = false;

                    while let Some(chunk_result) = stream.next().await {
                        if let Some(token) = &cancel_token {
                            if token.load(Ordering::Relaxed) {
                                let _ = file.flush().await;
                                return Err(anyhow::anyhow!("Cancelled by user"));
                            }
                        }

                        match chunk_result {
                            Ok(chunk) => {
                                if let Err(e) = file.write_all(&chunk).await {
                                    println!("Disk write error: {}", e);
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
                                        (downloaded_len as f32 / total_size as f32) * 100.0
                                    } else {
                                        0.0
                                    };

                                    progress_callback(pct, speed_str);
                                    last_report = std::time::Instant::now();
                                    bytes_since_last_report = 0;
                                }
                            }
                            Err(e) => {
                                println!("Stream error: {}", e);
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
            Err(e) => {
                println!("Connection error: {}", e);
            }
        }

        if attempt >= MAX_RETRIES {
            anyhow::bail!("Failed to download after {} attempts: {}", MAX_RETRIES, url);
        }

        let wait = std::time::Duration::from_secs(2u64.pow(attempt.min(4) as u32));
        progress_callback(
            if total_size > 0 {
                (downloaded_len as f32 / total_size as f32) * 100.0
            } else {
                0.0
            },
            format!("Network error. Retrying in {}s...", wait.as_secs()),
        );
        tokio::time::sleep(wait).await;
    }

    tokio::fs::rename(&temp_destination, destination).await?;
    progress_callback(100.0, "Complete".to_string());
    Ok(())
}

fn format_speed(bytes_per_sec: f64) -> String {
    if bytes_per_sec > 1_000_000.0 {
        format!("{:.2} MB/s", bytes_per_sec / 1_000_000.0)
    } else if bytes_per_sec > 1_000.0 {
        format!("{:.2} KB/s", bytes_per_sec / 1_000.0)
    } else {
        format!("{:.0} B/s", bytes_per_sec)
    }
}
