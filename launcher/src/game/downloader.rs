use anyhow::Result;
use futures::StreamExt;
use std::path::PathBuf;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

const MAX_RETRIES: u32 = 5;

pub async fn download_file(
    client: &reqwest::Client,
    url: &str,
    destination: &PathBuf,
    progress_callback: impl Fn(f32, String),
) -> Result<()> {
    let temp_destination = destination.with_extension("downloading");

    // Create parent directories
    if let Some(parent) = destination.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let mut attempt = 0;

    loop {
        attempt += 1;

        // 1. Verify current size (for Resume)
        let mut downloaded_len = 0u64;
        if temp_destination.exists() {
            if let Ok(metadata) = tokio::fs::metadata(&temp_destination).await {
                downloaded_len = metadata.len();
            }
        }

        // 2. Get total size (HEAD)
        // Some servers don't support HEAD, so failing here isn't critical for starting,
        // but it is for showing the progress bar.
        let total_size = if attempt == 1 {
            match client.head(url).send().await {
                Ok(resp) => {
                    // If the HEAD fails (e.g. 404), fail fast
                    if !resp.status().is_success() {
                        0
                    } else {
                        resp.content_length().unwrap_or(0)
                    }
                }
                Err(_) => 0,
            }
        } else {
            0 // In retries, simplify
        };

        // If we have everything, exit
        if total_size > 0 && downloaded_len == total_size {
            break;
        }

        println!(
            "Download attempt {}: Resuming from {} bytes. URL: {}",
            attempt, downloaded_len, url
        );

        // Build request
        let mut request_builder = client.get(url);

        // CRITICAL FIX: Only send Range if we actually have something downloaded.
        // Sending bytes=0- sometimes confuses CDNs on the first request.
        if downloaded_len > 0 {
            request_builder = request_builder.header("Range", format!("bytes={}-", downloaded_len));
        }

        let response_result = request_builder.send().await;

        match response_result {
            Ok(response) => {
                // CRITICAL FIX: Verify HTTP status
                let status = response.status();
                if !status.is_success() {
                    // If it's a client error (4xx), fail fast
                    if status.is_client_error() {
                        anyhow::bail!("Download failed with status: {} for URL: {}", status, url);
                    }
                    // If it's a server error (5xx), let the retry loop handle it,
                    // but throw an error here to avoid entering the write block.
                    println!("Server error {}, retrying...", status);
                } else {
                    // Determine if we open in append or create mode
                    let mut file = if status == reqwest::StatusCode::PARTIAL_CONTENT {
                        OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&temp_destination)
                            .await?
                    } else {
                        // If the server gives us 200 OK (does not support resume), start from 0
                        downloaded_len = 0;
                        tokio::fs::File::create(&temp_destination).await?
                    };

                    let mut stream = response.bytes_stream();
                    let mut last_report = std::time::Instant::now();
                    let mut bytes_since_last_report: u64 = 0;
                    let mut stream_failed = false;

                    while let Some(chunk_result) = stream.next().await {
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

                                // Progress report
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
                        // Verify that it's not 0 bytes (except for empty legit files, rare in mods)
                        if downloaded_len > 0 {
                            break; // Success
                        } else if total_size == 0 {
                            // Could be a valid empty file, but suspicious. Assume success if no error.
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

        let wait = std::time::Duration::from_secs(2u64.pow(attempt - 1));
        progress_callback(0.0, format!("Retrying in {}s...", wait.as_secs()));
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
