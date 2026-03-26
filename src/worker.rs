use std::sync::Arc;
use tokio::sync::broadcast;

use crate::AppState;
use crate::rename;

#[derive(Clone, Debug, serde::Serialize)]
pub struct ProgressUpdate {
    pub queue_id: i64,
    pub progress: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

pub fn spawn_worker(
    state: AppState,
    progress_tx: broadcast::Sender<ProgressUpdate>,
) -> tokio::task::JoinHandle<()> {
    let semaphore = Arc::new(tokio::sync::Semaphore::new(state.config.ytdlp.max_concurrent));

    tokio::spawn(async move {
        loop {
            // Poll for pending items
            let pending: Vec<(i64, String, Option<i64>)> = match sqlx::query_as(
                "SELECT id, url, category_id FROM queue WHERE status = 'pending' ORDER BY position ASC, created_at ASC LIMIT 5"
            )
            .fetch_all(&state.db)
            .await
            {
                Ok(rows) => rows,
                Err(e) => {
                    tracing::error!("Failed to poll queue: {}", e);
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
            };

            if pending.is_empty() {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                continue;
            }

            for (id, url, category_id) in pending {
                let permit = semaphore.clone().acquire_owned().await.unwrap();
                let state = state.clone();
                let tx = progress_tx.clone();

                tokio::spawn(async move {
                    download_item(&state, id, &url, category_id, &tx).await;
                    drop(permit);
                });
            }

            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    })
}

async fn download_item(
    state: &AppState,
    queue_id: i64,
    url: &str,
    category_id: Option<i64>,
    tx: &broadcast::Sender<ProgressUpdate>,
) {
    // Mark as downloading
    if let Err(e) = sqlx::query("UPDATE queue SET status = 'downloading', updated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(queue_id)
        .execute(&state.db)
        .await
    {
        tracing::error!("Failed to update queue status for {}: {}", queue_id, e);
        return;
    }

    let _ = tx.send(ProgressUpdate {
        queue_id,
        progress: "starting".to_string(),
        status: "downloading".to_string(),
        output: None,
    });

    let download_dir = &state.config.storage.download_dir;

    // Resolve category name if set
    let category_name: Option<String> = if let Some(cat_id) = category_id {
        sqlx::query_scalar("SELECT name FROM categories WHERE id = ?")
            .bind(cat_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten()
    } else {
        None
    };

    // Build output path: [download_dir]/[category]/[host]/%(title)s.%(ext)s
    let mut base = std::path::PathBuf::from(download_dir);
    if let Some(ref cat) = category_name {
        base.push(cat);
    }
    if state.config.storage.segment_by_host {
        let host = parse_host(url).unwrap_or_else(|| "unknown".to_string());
        base.push(&host);
    }
    let _ = std::fs::create_dir_all(&base);
    let output_template = format!("{}/%(title)s.%(ext)s", base.display());

    // Use PTY to capture output — yt-dlp behaves like it's in a terminal
    // (proper line buffering, progress output, color codes stripped by --newline)
    let pty = portable_pty::native_pty_system();
    let pty_pair = pty.openpty(portable_pty::PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }).expect("failed to open pty");

    let mut pty_cmd = portable_pty::CommandBuilder::new(&state.config.ytdlp.binary);
    pty_cmd.args(["--newline", "--continue", "--print", "after_move:filepath"]);
    pty_cmd.args(["-o", &output_template]);
    for arg in &state.config.ytdlp.extra_args {
        pty_cmd.arg(arg);
    }
    pty_cmd.arg(url);

    let mut pty_child = match pty_pair.slave.spawn_command(pty_cmd) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to spawn yt-dlp for queue {}: {}", queue_id, e);
            let _ = sqlx::query("UPDATE queue SET status = 'failed', error = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
                .bind(format!("Failed to spawn yt-dlp: {}", e))
                .bind(queue_id)
                .execute(&state.db)
                .await;
            let _ = tx.send(ProgressUpdate {
                queue_id,
                progress: String::new(),
                status: "failed".to_string(),
                output: Some(format!("Failed to spawn yt-dlp: {}", e)),
            });
            return;
        }
    };
    // Drop the slave so reads on the master will see EOF when the child exits
    drop(pty_pair.slave);

    // Read from PTY master in a blocking thread (PTY reads are blocking)
    let reader = pty_pair.master.try_clone_reader().expect("failed to clone pty reader");
    let pty_tx = tx.clone();
    let pty_db = state.db.clone();
    let pty_queue_id = queue_id;
    let mut final_filepath: Option<String> = None;

    let read_handle = tokio::task::spawn_blocking(move || {
        let mut buf_reader = std::io::BufReader::new(reader);
        let mut line_buf = String::new();
        let mut filepath: Option<String> = None;

        loop {
            line_buf.clear();
            // Read raw bytes, splitting on \n (PTY converts \r\n)
            let mut byte = [0u8; 1];
            loop {
                match std::io::Read::read(&mut buf_reader, &mut byte) {
                    Ok(0) => return filepath, // EOF
                    Ok(_) => {
                        let ch = byte[0] as char;
                        if ch == '\n' {
                            break;
                        } else if ch == '\r' {
                            // Treat \r as line break too (progress lines)
                            if !line_buf.is_empty() {
                                break;
                            }
                        } else {
                            line_buf.push(ch);
                        }
                    }
                    Err(_) => return filepath, // Read error = done
                }
            }

            let line = line_buf.trim().to_string();
            if line.is_empty() {
                continue;
            }

            if line.starts_with("[download]") {
                if let Some(pct) = parse_progress(&line) {
                    // Update DB progress (blocking context, use Handle)
                    let db = pty_db.clone();
                    let pct_clone = pct.clone();
                    let _ = tokio::runtime::Handle::current().block_on(async {
                        sqlx::query("UPDATE queue SET progress = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
                            .bind(&pct_clone)
                            .bind(pty_queue_id)
                            .execute(&db)
                            .await
                    });
                    let _ = pty_tx.send(ProgressUpdate {
                        queue_id: pty_queue_id,
                        progress: pct,
                        status: "downloading".to_string(),
                        output: Some(line),
                    });
                } else {
                    let _ = pty_tx.send(ProgressUpdate {
                        queue_id: pty_queue_id,
                        progress: String::new(),
                        status: "downloading".to_string(),
                        output: Some(line),
                    });
                }
            } else if line.starts_with('[') || line.starts_with("WARNING") || line.starts_with("ERROR") {
                let _ = pty_tx.send(ProgressUpdate {
                    queue_id: pty_queue_id,
                    progress: String::new(),
                    status: "downloading".to_string(),
                    output: Some(line),
                });
            } else {
                // Lines without brackets are likely the --print output (final filepath)
                filepath = Some(line.clone());
                let _ = pty_tx.send(ProgressUpdate {
                    queue_id: pty_queue_id,
                    progress: String::new(),
                    status: "downloading".to_string(),
                    output: Some(line),
                });
            }
        }
    });

    // Wait for both the reader and the child process
    if let Ok(fp) = read_handle.await {
        final_filepath = fp;
    }
    let status = pty_child.wait();

    match status {
        Ok(exit) if exit.success() => {
            handle_success(state, queue_id, category_id, final_filepath, tx).await;
        }
        Ok(_) | Err(_) => {
            handle_failure(state, queue_id, category_id, final_filepath, download_dir, tx).await;
        }
    }
}

async fn handle_success(
    state: &AppState,
    queue_id: i64,
    category_id: Option<i64>,
    final_filepath: Option<String>,
    tx: &broadcast::Sender<ProgressUpdate>,
) {
    if let Some(ref filepath) = final_filepath {
        let path = std::path::Path::new(filepath);
        let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let size_bytes = std::fs::metadata(filepath).map(|m| m.len() as i64).ok();

        let _ = sqlx::query(
            "INSERT INTO files (queue_id, filename, original, path, size_bytes, category_id) VALUES (?, ?, ?, ?, ?, ?)"
        )
        .bind(queue_id)
        .bind(&filename)
        .bind(&filename)
        .bind(filepath)
        .bind(size_bytes)
        .bind(category_id)
        .execute(&state.db)
        .await;
    }

    let _ = sqlx::query("UPDATE queue SET status = 'completed', progress = '100%', updated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(queue_id)
        .execute(&state.db)
        .await;

    let _ = tx.send(ProgressUpdate {
        queue_id,
        progress: "100%".to_string(),
        status: "completed".to_string(),
        output: Some("Download completed".to_string()),
    });

    tracing::info!("Download completed for queue {}", queue_id);
}

async fn handle_failure(
    state: &AppState,
    queue_id: i64,
    category_id: Option<i64>,
    final_filepath: Option<String>,
    download_dir: &str,
    tx: &broadcast::Sender<ProgressUpdate>,
) {
    // Check if a large file was left behind (incomplete download)
    if let Some(ref filepath) = final_filepath {
        if let Ok(meta) = std::fs::metadata(filepath) {
            if meta.len() > 1_000_000 {
                // File is > 1MB, treat as incomplete
                let path = std::path::Path::new(filepath);
                let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                let incomplete_name = rename::incomplete_filename(&filename);
                let file_dir = path.parent().unwrap_or(std::path::Path::new(download_dir));
                let incomplete_path = file_dir.join(&incomplete_name);

                if let Ok(()) = std::fs::rename(filepath, &incomplete_path) {
                    let _ = sqlx::query(
                        "INSERT INTO files (queue_id, filename, original, path, size_bytes, category_id) VALUES (?, ?, ?, ?, ?, ?)"
                    )
                    .bind(queue_id)
                    .bind(&incomplete_name)
                    .bind(&filename)
                    .bind(incomplete_path.to_string_lossy().as_ref())
                    .bind(meta.len() as i64)
                    .bind(category_id)
                    .execute(&state.db)
                    .await;

                    tracing::warn!("Download failed for queue {} but saved as incomplete: {}", queue_id, incomplete_name);
                }
            }
        }
    }

    // Also check for .part files in the download dir
    // yt-dlp with --continue will resume from these

    let _ = sqlx::query("UPDATE queue SET status = 'failed', error = 'Download failed', updated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(queue_id)
        .execute(&state.db)
        .await;

    let _ = tx.send(ProgressUpdate {
        queue_id,
        progress: String::new(),
        status: "failed".to_string(),
        output: Some("Download failed".to_string()),
    });

    tracing::error!("Download failed for queue {}", queue_id);
}

fn parse_host(url: &str) -> Option<String> {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
}

fn parse_progress(line: &str) -> Option<String> {
    // [download]  45.2% of ~50.00MiB at  5.00MiB/s ETA 00:05
    // [download] 100% of 50.00MiB
    if let Some(pct_end) = line.find('%') {
        let before = &line[..pct_end];
        let pct_start = before.rfind(|c: char| !c.is_ascii_digit() && c != '.')?;
        let pct = before[pct_start + 1..].trim();
        if !pct.is_empty() {
            return Some(format!("{}%", pct));
        }
    }
    None
}

/// Reset any downloads that were in progress when the server shut down
pub async fn reset_interrupted_downloads(db: &sqlx::SqlitePool) {
    let result = sqlx::query("UPDATE queue SET status = 'pending', progress = NULL, updated_at = CURRENT_TIMESTAMP WHERE status = 'downloading'")
        .execute(db)
        .await;

    match result {
        Ok(r) if r.rows_affected() > 0 => {
            tracing::info!("Reset {} interrupted downloads to pending", r.rows_affected());
        }
        Ok(_) => {}
        Err(e) => {
            tracing::error!("Failed to reset interrupted downloads: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_progress() {
        assert_eq!(
            parse_progress("[download]  45.2% of ~50.00MiB at  5.00MiB/s ETA 00:05"),
            Some("45.2%".to_string())
        );
        assert_eq!(
            parse_progress("[download] 100% of 50.00MiB"),
            Some("100%".to_string())
        );
        assert_eq!(
            parse_progress("[download]   0.5% of ~50.00MiB"),
            Some("0.5%".to_string())
        );
        assert_eq!(
            parse_progress("[download] Destination: some file.mp4"),
            None
        );
    }

    #[test]
    fn test_parse_host() {
        assert_eq!(parse_host("https://www.youtube.com/watch?v=abc"), Some("www.youtube.com".to_string()));
        assert_eq!(parse_host("https://vimeo.com/12345"), Some("vimeo.com".to_string()));
        assert_eq!(parse_host("https://x.com/user/status/123"), Some("x.com".to_string()));
        assert_eq!(parse_host("not-a-url"), None);
    }
}
