use std::sync::Arc;
use tokio::io::AsyncBufReadExt;
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

    let mut cmd = tokio::process::Command::new(&state.config.ytdlp.binary);
    cmd.arg("--newline")
        .arg("--continue")
        .arg("--print").arg("after_move:filepath")
        .arg("-o").arg(&output_template);

    for arg in &state.config.ytdlp.extra_args {
        cmd.arg(arg);
    }

    cmd.arg(url)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = match cmd.spawn() {
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

    // Stream stderr in a separate task
    let stderr = child.stderr.take().unwrap();
    let stderr_tx = tx.clone();
    let stderr_queue_id = queue_id;
    tokio::spawn(async move {
        let mut reader = tokio::io::BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let line = line.trim().to_string();
            if line.is_empty() { continue; }
            let _ = stderr_tx.send(ProgressUpdate {
                queue_id: stderr_queue_id,
                progress: String::new(),
                status: "downloading".to_string(),
                output: Some(line),
            });
        }
    });

    // Parse stdout for progress, output lines, and final filepath
    let stdout = child.stdout.take().unwrap();
    let mut reader = tokio::io::BufReader::new(stdout).lines();
    let mut final_filepath: Option<String> = None;

    while let Ok(Some(line)) = reader.next_line().await {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        // yt-dlp --print after_move:filepath outputs the final path as the last line
        // Progress lines look like: [download]  45.2% of ~50.00MiB at  5.00MiB/s ETA 00:05
        if line.starts_with("[download]") {
            if let Some(pct) = parse_progress(&line) {
                let _ = sqlx::query("UPDATE queue SET progress = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
                    .bind(&pct)
                    .bind(queue_id)
                    .execute(&state.db)
                    .await;
                let _ = tx.send(ProgressUpdate {
                    queue_id,
                    progress: pct,
                    status: "downloading".to_string(),
                    output: Some(line),
                });
            } else {
                // Non-progress download line (e.g. "Destination: ...")
                let _ = tx.send(ProgressUpdate {
                    queue_id,
                    progress: String::new(),
                    status: "downloading".to_string(),
                    output: Some(line),
                });
            }
        } else if line.starts_with('[') {
            // Other yt-dlp status lines like [info], [ExtractAudio], [Merger], etc.
            let _ = tx.send(ProgressUpdate {
                queue_id,
                progress: String::new(),
                status: "downloading".to_string(),
                output: Some(line),
            });
        } else {
            // Lines without brackets are likely the --print output (final filepath)
            final_filepath = Some(line);
        }
    }

    // Wait for process to exit
    let status = child.wait().await;

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
