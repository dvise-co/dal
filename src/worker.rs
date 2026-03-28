use std::sync::Arc;
use tokio::io::AsyncBufReadExt;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

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
    cancel: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    let semaphore = Arc::new(tokio::sync::Semaphore::new(
        state.config.ytdlp.max_concurrent,
    ));

    tokio::spawn(async move {
        loop {
            if cancel.is_cancelled() {
                tracing::info!("Worker shutting down");
                return;
            }

            let pending: Vec<(i64, String, Option<i64>)> = match sqlx::query_as(
                "SELECT id, url, category_id FROM queue WHERE status = 'pending' ORDER BY position ASC, created_at ASC LIMIT 5"
            )
            .fetch_all(&state.db)
            .await
            {
                Ok(rows) => rows,
                Err(e) => {
                    tracing::error!("Failed to poll queue: {}", e);
                    tokio::select! {
                        _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {}
                        _ = cancel.cancelled() => return
                    }
                    continue;
                }
            };

            if pending.is_empty() {
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {}
                    _ = cancel.cancelled() => return
                }
                continue;
            }

            for (id, url, category_id) in pending {
                let permit = semaphore.clone().acquire_owned().await.unwrap();
                let state = state.clone();
                let cancel = cancel.clone();
                let tx = progress_tx.clone();

                tokio::spawn(async move {
                    download_item(&state, id, &url, category_id, cancel, &tx).await;
                    drop(permit);
                });
            }

            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
                _ = cancel.cancelled() => return
            }
        }
    })
}

async fn download_item(
    state: &AppState,
    queue_id: i64,
    url: &str,
    category_id: Option<i64>,
    cancel: CancellationToken,
    tx: &broadcast::Sender<ProgressUpdate>,
) {
    tracing::info!("Starting download for queue {} url={}", queue_id, url);

    // Mark as downloading and increment attempts
    if let Err(e) = sqlx::query(
        "UPDATE queue SET status = 'downloading', attempts = COALESCE(attempts, 0) + 1, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
    )
    .bind(queue_id)
    .execute(&state.db)
    .await
    {
        tracing::error!("Failed to update queue status for {}: {}", queue_id, e);
        return;
    }

    // Record history
    let _ = sqlx::query("INSERT INTO queue_history (queue_id, status) VALUES (?, 'downloading')")
        .bind(queue_id)
        .execute(&state.db)
        .await;

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

    let ytdlp = &state.config.ytdlp;

    let output_template = match &ytdlp.output_template {
        Some(tpl) => format!("{}/{}", base.display(), tpl),
        None => format!("{}/%(title)s.%(ext)s", base.display()),
    };

    let mut cmd = tokio::process::Command::new(&ytdlp.binary);

    // Default args from config (e.g. --newline, --verbose, --continue)
    for arg in &ytdlp.default_args {
        cmd.arg(arg);
    }

    // Internal flags required for the worker
    cmd.arg("--print")
        .arg("after_move:filepath")
        .arg("-o")
        .arg(&output_template);

    // Extra args from config
    for arg in &ytdlp.extra_args {
        cmd.arg(arg);
    }
    cmd.arg(url)
        .env("PYTHONUNBUFFERED", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    tracing::debug!(
        "Spawning yt-dlp for queue {}: {} -o {}",
        queue_id,
        &state.config.ytdlp.binary,
        &output_template
    );

    let mut child = match cmd.spawn() {
        Ok(c) => {
            tracing::info!("yt-dlp spawned for queue {} (pid: {:?})", queue_id, c.id());
            c
        }
        Err(e) => {
            tracing::error!("Failed to spawn yt-dlp for queue {}: {}", queue_id, e);
            let _ = sqlx::query("UPDATE queue SET status = 'failed', error = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
                .bind(format!("Failed to spawn yt-dlp: {}", e))
                .bind(queue_id)
                .execute(&state.db)
                .await;
            return;
        }
    };

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let mut final_filepath: Option<String> = None;
    let mut output_log = String::new();
    let mut reached_100_pct = false;

    // Stream stderr lines into output_log via a shared buffer
    let db_stderr = state.db.clone();
    let stderr_output = Arc::new(tokio::sync::Mutex::new(String::new()));
    let stderr_output_clone = stderr_output.clone();
    let stderr_queue_id = queue_id;
    let stderr_tx = tx.clone();

    let stderr_handle = tokio::spawn(async move {
        let mut reader = tokio::io::BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }
            tracing::info!("yt-dlp stderr [{}]: {}", stderr_queue_id, &line);
            let mut log = stderr_output_clone.lock().await;
            if !log.is_empty() {
                log.push('\n');
            }
            log.push_str(&line);
            let _ = sqlx::query(
                "UPDATE queue SET output = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            )
            .bind(log.as_str())
            .bind(stderr_queue_id)
            .execute(&db_stderr)
            .await;
            let _ = stderr_tx.send(ProgressUpdate {
                queue_id: stderr_queue_id,
                progress: String::new(),
                status: "downloading".to_string(),
                output: Some(line),
            });
        }
    });

    // Read stdout for progress and final filepath
    let mut reader = tokio::io::BufReader::new(stdout).lines();

    loop {
        tokio::select! {
            line_result = reader.next_line() => {
                match line_result {
                    Ok(Some(line)) => {
                        let line = line.trim().to_string();
                        if line.is_empty() { continue; }

                        tracing::info!("yt-dlp stdout [{}]: {}", queue_id, &line);

                        // Append to output log
                        if !output_log.is_empty() { output_log.push('\n'); }
                        output_log.push_str(&line);

                        // Trim to last 200 lines
                        let line_count = output_log.chars().filter(|c| *c == '\n').count();
                        if line_count > 200 && let Some(pos) = output_log.find('\n') {
                            output_log = output_log[pos + 1..].to_string();
                        }

                        // Parse progress percentage if present
                        if let Some(pct) = parse_progress(&line) {
                            if pct == "100%" {
                                reached_100_pct = true;
                            }
                            let _ = sqlx::query("UPDATE queue SET progress = ?, output = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
                                .bind(&pct)
                                .bind(&output_log)
                                .bind(queue_id)
                                .execute(&state.db)
                                .await;
                            let _ = tx.send(ProgressUpdate {
                                queue_id,
                                progress: pct,
                                status: "downloading".to_string(),
                                output: Some(line),
                            });
                        } else if line.starts_with('[') || line.starts_with("WARNING") || line.starts_with("ERROR") || line.starts_with("DEBUG") {
                            let _ = sqlx::query("UPDATE queue SET output = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
                                .bind(&output_log)
                                .bind(queue_id)
                                .execute(&state.db)
                                .await;
                            let _ = tx.send(ProgressUpdate {
                                queue_id,
                                progress: String::new(),
                                status: "downloading".to_string(),
                                output: Some(line),
                            });
                        } else {
                            // --print output (final filepath)
                            final_filepath = Some(line);
                        }
                    }
                    Ok(None) => {
                        tracing::info!("yt-dlp stdout EOF for queue {}", queue_id);
                        break;
                    }
                    Err(e) => {
                        tracing::error!("yt-dlp stdout read error for queue {}: {}", queue_id, e);
                        break;
                    }
                }
            }
            _ = cancel.cancelled() => {
                tracing::info!("Killing yt-dlp for queue {} due to shutdown", queue_id);
                let _ = child.kill().await;
                return;
            }
        }
    }

    // Merge any stderr output
    {
        let stderr_log = stderr_output.lock().await;
        if !stderr_log.is_empty() {
            if !output_log.is_empty() {
                output_log.push('\n');
            }
            output_log.push_str(&stderr_log);
            let _ = sqlx::query(
                "UPDATE queue SET output = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            )
            .bind(&output_log)
            .bind(queue_id)
            .execute(&state.db)
            .await;
        }
    }

    let _ = stderr_handle.await;
    let status = child.wait().await;
    tracing::info!(
        "yt-dlp exited for queue {}: {:?}, filepath={:?}",
        queue_id,
        status,
        final_filepath
    );

    // Check for provisional success:
    // 1. Progress reached 100% and file exists (>1KB)
    // 2. File exists and is large (>100MB) regardless of progress
    let file_size = final_filepath
        .as_ref()
        .and_then(|fp| std::fs::metadata(fp).ok().map(|m| m.len()));

    let is_provisional = !matches!(&status, Ok(exit) if exit.success())
        && file_size.is_some_and(|size| (reached_100_pct && size > 1_000) || size > 100_000_000);

    let provisional_reason = if is_provisional {
        if reached_100_pct {
            Some("Provisional success: yt-dlp reported error but download reached 100%")
        } else {
            Some("Provisional success: yt-dlp reported error but file is >100MB")
        }
    } else {
        None
    };

    match status {
        Ok(exit) if exit.success() => {
            record_history(&state.db, queue_id, "completed", None).await;
            handle_success(state, queue_id, url, category_id, final_filepath, false).await;
        }
        _ if is_provisional => {
            tracing::warn!(
                "yt-dlp exited with error for queue {} — treating as provisional success: {}",
                queue_id,
                provisional_reason.unwrap_or("unknown")
            );
            record_history(&state.db, queue_id, "completed", provisional_reason).await;
            handle_success(state, queue_id, url, category_id, final_filepath, true).await;
        }
        Ok(_) | Err(_) => {
            record_history(&state.db, queue_id, "failed", None).await;
            handle_failure(
                state,
                queue_id,
                url,
                category_id,
                final_filepath,
                download_dir,
            )
            .await;
        }
    }
}

async fn record_history(db: &sqlx::SqlitePool, queue_id: i64, status: &str, error: Option<&str>) {
    let _ = sqlx::query("INSERT INTO queue_history (queue_id, status, error) VALUES (?, ?, ?)")
        .bind(queue_id)
        .bind(status)
        .bind(error)
        .execute(db)
        .await;
}

async fn handle_success(
    state: &AppState,
    queue_id: i64,
    url: &str,
    category_id: Option<i64>,
    final_filepath: Option<String>,
    provisional: bool,
) {
    if let Some(ref filepath) = final_filepath {
        let path = std::path::Path::new(filepath);
        let filename = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let size_bytes = std::fs::metadata(filepath).map(|m| m.len() as i64).ok();
        let relative = state.config.storage.to_relative_path(filepath);

        let _ = sqlx::query(
            "INSERT INTO files (queue_id, url, filename, original, path, size_bytes, category_id, provisional) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(queue_id)
        .bind(url)
        .bind(&filename)
        .bind(&filename)
        .bind(&relative)
        .bind(size_bytes)
        .bind(category_id)
        .bind(provisional)
        .execute(&state.db)
        .await;
    }

    let _ = sqlx::query("UPDATE queue SET status = 'completed', progress = '100%', updated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(queue_id)
        .execute(&state.db)
        .await;

    tracing::info!("Download completed for queue {}", queue_id);
}

async fn handle_failure(
    state: &AppState,
    queue_id: i64,
    url: &str,
    category_id: Option<i64>,
    final_filepath: Option<String>,
    download_dir: &str,
) {
    if let Some(ref filepath) = final_filepath
        && let Ok(meta) = std::fs::metadata(filepath)
        && meta.len() > 1_000_000
    {
        let path = std::path::Path::new(filepath);
        let filename = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let incomplete_name = rename::incomplete_filename(&filename);
        let file_dir = path.parent().unwrap_or(std::path::Path::new(download_dir));
        let incomplete_path = file_dir.join(&incomplete_name);

        if let Ok(()) = std::fs::rename(filepath, &incomplete_path) {
            let relative = state
                .config
                .storage
                .to_relative_path(&incomplete_path.to_string_lossy());
            let _ = sqlx::query(
                "INSERT INTO files (queue_id, url, filename, original, path, size_bytes, category_id) VALUES (?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(queue_id)
            .bind(url)
            .bind(&incomplete_name)
            .bind(&filename)
            .bind(&relative)
            .bind(meta.len() as i64)
            .bind(category_id)
            .execute(&state.db)
            .await;

            tracing::warn!(
                "Download failed for queue {} but saved as incomplete: {}",
                queue_id,
                incomplete_name
            );
        }
    }

    let _ = sqlx::query("UPDATE queue SET status = 'failed', error = 'Download failed', updated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(queue_id)
        .execute(&state.db)
        .await;

    tracing::error!("Download failed for queue {}", queue_id);
}

#[allow(dead_code)]
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            if let Some(next) = chars.next()
                && next == '['
            {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            result.push(ch);
        }
    }
    result
}

fn parse_host(url: &str) -> Option<String> {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
}

fn parse_progress(line: &str) -> Option<String> {
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

pub async fn reset_interrupted_downloads(db: &sqlx::SqlitePool) {
    let result = sqlx::query("UPDATE queue SET status = 'pending', progress = NULL, updated_at = CURRENT_TIMESTAMP WHERE status = 'downloading'")
        .execute(db)
        .await;

    match result {
        Ok(r) if r.rows_affected() > 0 => {
            tracing::info!(
                "Reset {} interrupted downloads to pending",
                r.rows_affected()
            );
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
        assert_eq!(
            parse_host("https://www.youtube.com/watch?v=abc"),
            Some("www.youtube.com".to_string())
        );
        assert_eq!(
            parse_host("https://vimeo.com/12345"),
            Some("vimeo.com".to_string())
        );
        assert_eq!(
            parse_host("https://x.com/user/status/123"),
            Some("x.com".to_string())
        );
        assert_eq!(parse_host("not-a-url"), None);
    }

    #[test]
    fn test_strip_ansi() {
        assert_eq!(
            strip_ansi("\x1b[0;94m[download]\x1b[0m  45.2%"),
            "[download]  45.2%"
        );
        assert_eq!(strip_ansi("no escape codes"), "no escape codes");
        assert_eq!(strip_ansi(""), "");
    }
}
