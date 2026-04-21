use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

mod api;
mod config;
mod db;
mod rename;
mod worker;

#[derive(Parser)]
#[command(name = "dal", about = "DAL — Download, Assess, Label")]
struct Cli {
    /// Path to configuration file
    #[arg(short, long, default_value = "./dal.toml")]
    config: PathBuf,
}

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::SqlitePool,
    pub config: Arc<config::Config>,
    pub progress_tx: broadcast::Sender<worker::ProgressUpdate>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "dal=info,tower_http=debug".into()),
        )
        .init();

    let cli = Cli::parse();
    let config = config::load_config(&cli.config)?;

    tracing::info!("DAL server starting");
    tracing::info!("Download dir: {}", config.storage.download_dir);
    tracing::info!("Database: {}", config.storage.database);

    // Ensure download directory exists
    std::fs::create_dir_all(&config.storage.download_dir)?;

    // Initialize database
    let pool = db::init_db(&config.storage.database).await?;

    // Migrate any absolute paths in the files table to relative paths
    migrate_to_relative_paths(&pool, &config.storage).await;

    // Strip the per-host subfolder from stored paths and move files on disk
    migrate_remove_host_subfolder(&pool, &config.storage).await;

    // Reset any interrupted downloads from previous run
    worker::reset_interrupted_downloads(&pool).await;

    // Broadcast channel for real-time progress via WebSocket
    let (progress_tx, _) = broadcast::channel::<worker::ProgressUpdate>(256);

    let addr = format!("{}:{}", config.server.host, config.server.port);
    let state = AppState {
        db: pool,
        config: Arc::new(config),
        progress_tx: progress_tx.clone(),
    };

    // Spawn download worker with cancellation token
    let cancel = CancellationToken::new();
    let worker_handle = worker::spawn_worker(state.clone(), progress_tx, cancel.clone());

    let app = api::router()
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Listening on {}", addr);

    let shutdown = async {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("Shutdown signal received, stopping...");
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await?;

    cancel.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), worker_handle).await;

    worker::reset_interrupted_downloads(&state.db).await;

    tracing::info!("DAL server stopped");
    std::process::exit(0);
}

/// Migrate existing absolute paths to relative paths by stripping known prefixes.
/// Handles both download_dir and display_path_prefix in case either leaked into the DB.
async fn migrate_to_relative_paths(db: &sqlx::SqlitePool, storage: &config::StorageConfig) {
    let mut prefixes = vec![storage.download_dir.clone()];
    if let Some(ref display) = storage.display_path_prefix {
        prefixes.push(display.clone());
    }

    let mut total = 0u64;
    for raw_prefix in &prefixes {
        let prefix = format!("{}/", raw_prefix.trim_end_matches('/'));
        let pattern = format!("{}%", prefix);

        match sqlx::query("UPDATE files SET path = SUBSTR(path, ?) WHERE path LIKE ?")
            .bind(prefix.len() as i64 + 1) // SUBSTR is 1-indexed
            .bind(&pattern)
            .execute(db)
            .await
        {
            Ok(r) => total += r.rows_affected(),
            Err(e) => {
                tracing::error!(
                    "Failed to migrate file paths for prefix '{}': {}",
                    prefix,
                    e
                );
            }
        }
    }

    if total > 0 {
        tracing::info!("Migrated {} file paths from absolute to relative", total);
    }
}

/// Backfill `queue.host` from `queue.url`, then strip the per-host subfolder from
/// existing `files` rows, moving the files on disk. Idempotent — rows whose path
/// no longer contains a host segment are left untouched.
async fn migrate_remove_host_subfolder(db: &sqlx::SqlitePool, storage: &config::StorageConfig) {
    // Backfill queue.host from queue.url for any rows still missing it.
    let pending_hosts: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, url FROM queue WHERE host IS NULL")
            .fetch_all(db)
            .await
            .unwrap_or_default();

    let mut backfilled = 0u64;
    for (qid, url) in pending_hosts {
        if let Some(host) = worker::parse_host(&url)
            && sqlx::query("UPDATE queue SET host = ? WHERE id = ?")
                .bind(&host)
                .bind(qid)
                .execute(db)
                .await
                .is_ok()
        {
            backfilled += 1;
        }
    }
    if backfilled > 0 {
        tracing::info!("Backfilled host on {} queue rows", backfilled);
    }

    // Rewrite file paths to drop the host segment.
    let files: Vec<(i64, String, Option<i64>, Option<i64>)> =
        sqlx::query_as("SELECT id, path, queue_id, category_id FROM files")
            .fetch_all(db)
            .await
            .unwrap_or_default();

    let mut moved = 0u64;
    let mut updated_db_only = 0u64;
    let mut collided = 0u64;

    for (file_id, old_rel, queue_id, category_id) in files {
        let queue_host: Option<String> = match queue_id {
            Some(qid) => sqlx::query_scalar("SELECT host FROM queue WHERE id = ?")
                .bind(qid)
                .fetch_optional(db)
                .await
                .ok()
                .flatten(),
            None => None,
        };

        let Some((new_rel, host_segment)) =
            strip_host_segment(&old_rel, category_id.is_some(), queue_host.as_deref())
        else {
            continue;
        };

        let old_abs = storage.full_path(&old_rel);
        let new_abs = storage.full_path(&new_rel);
        let old_path = std::path::Path::new(&old_abs);

        if !old_path.exists() {
            // File is gone (already moved, deleted, or scan placeholder) — just fix the DB.
            let _ = sqlx::query("UPDATE files SET path = ? WHERE id = ?")
                .bind(&new_rel)
                .bind(file_id)
                .execute(db)
                .await;
            updated_db_only += 1;
        } else if std::path::Path::new(&new_abs).exists() {
            // Collision at the destination — suffix with the host before the extension.
            let filename = std::path::Path::new(&new_rel)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let collision_name = insert_host_before_ext(&filename, &host_segment);
            let collision_rel = match std::path::Path::new(&new_rel).parent() {
                Some(p) if !p.as_os_str().is_empty() => {
                    format!("{}/{}", p.to_string_lossy(), collision_name)
                }
                _ => collision_name.clone(),
            };
            let collision_abs = storage.full_path(&collision_rel);
            match std::fs::rename(&old_abs, &collision_abs) {
                Ok(_) => {
                    let _ = sqlx::query(
                        "UPDATE files SET path = ?, filename = ? WHERE id = ?",
                    )
                    .bind(&collision_rel)
                    .bind(&collision_name)
                    .bind(file_id)
                    .execute(db)
                    .await;
                    collided += 1;
                    moved += 1;
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to move {} -> {} (collision path): {}",
                        old_abs,
                        collision_abs,
                        e
                    );
                    continue;
                }
            }
        } else {
            if let Some(parent) = std::path::Path::new(&new_abs).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match std::fs::rename(&old_abs, &new_abs) {
                Ok(_) => {
                    let _ = sqlx::query("UPDATE files SET path = ? WHERE id = ?")
                        .bind(&new_rel)
                        .bind(file_id)
                        .execute(db)
                        .await;
                    moved += 1;
                }
                Err(e) => {
                    tracing::error!("Failed to move {} -> {}: {}", old_abs, new_abs, e);
                    continue;
                }
            }
        }

        // Attempt to remove the now-empty host directory. Ignore errors (not empty, etc).
        if let Some(parent) = old_path.parent() {
            let _ = std::fs::remove_dir(parent);
        }
    }

    if moved + updated_db_only > 0 {
        tracing::info!(
            "Removed host subfolder: moved {} files ({} collisions), fixed {} DB-only rows",
            moved,
            collided,
            updated_db_only
        );
    }
}

/// Given a stored relative path, return the path with the host segment removed
/// plus the host segment itself, or None if no host segment is present.
///
/// `has_category` indicates whether the path is expected to start with a category
/// segment. `expected_host` is the host recorded on the associated queue row, if any —
/// when present it must match the candidate segment exactly. Without it we fall back
/// to a heuristic (segment contains a '.' — i.e. looks like a hostname).
fn strip_host_segment(
    rel: &str,
    has_category: bool,
    expected_host: Option<&str>,
) -> Option<(String, String)> {
    let trimmed = rel.trim_start_matches('/');
    let parts: Vec<&str> = trimmed.split('/').collect();

    let host_idx = if has_category {
        if parts.len() < 3 {
            return None;
        }
        1
    } else {
        if parts.len() < 2 {
            return None;
        }
        0
    };

    let candidate = parts[host_idx];
    let is_host = match expected_host {
        Some(h) => candidate == h,
        None => looks_like_host(candidate),
    };
    if !is_host {
        return None;
    }

    let mut remaining: Vec<&str> = Vec::with_capacity(parts.len() - 1);
    for (i, p) in parts.iter().enumerate() {
        if i != host_idx {
            remaining.push(p);
        }
    }
    Some((remaining.join("/"), candidate.to_string()))
}

fn looks_like_host(s: &str) -> bool {
    s.contains('.') && !s.is_empty()
}

/// Insert `host` before the final `.ext` in `filename`. Files with no extension
/// get the host appended with a separating dot.
fn insert_host_before_ext(filename: &str, host: &str) -> String {
    match filename.rfind('.') {
        Some(i) if i > 0 && i < filename.len() - 1 => {
            format!("{}.{}{}", &filename[..i], host, &filename[i..])
        }
        _ => format!("{}.{}", filename, host),
    }
}

#[cfg(test)]
mod migrate_tests {
    use super::*;

    #[test]
    fn strip_with_category_and_known_host() {
        let (new_rel, host) = strip_host_segment(
            "movies/www.youtube.com/clip.mp4",
            true,
            Some("www.youtube.com"),
        )
        .unwrap();
        assert_eq!(new_rel, "movies/clip.mp4");
        assert_eq!(host, "www.youtube.com");
    }

    #[test]
    fn strip_no_category_heuristic() {
        let (new_rel, host) = strip_host_segment("vimeo.com/clip.mp4", false, None).unwrap();
        assert_eq!(new_rel, "clip.mp4");
        assert_eq!(host, "vimeo.com");
    }

    #[test]
    fn strip_idempotent_when_no_host() {
        // Category path with no host subfolder — should not strip.
        assert!(strip_host_segment("movies/clip.mp4", true, None).is_none());
        // Single-segment path — nothing to strip.
        assert!(strip_host_segment("clip.mp4", false, None).is_none());
    }

    #[test]
    fn strip_rejects_mismatch_on_expected_host() {
        // Expected host mismatch — don't strip even if segment looks like a host.
        assert!(
            strip_host_segment(
                "movies/other.com/clip.mp4",
                true,
                Some("www.youtube.com"),
            )
            .is_none()
        );
    }

    #[test]
    fn insert_host_before_extension() {
        assert_eq!(
            insert_host_before_ext("clip.mp4", "www.youtube.com"),
            "clip.www.youtube.com.mp4"
        );
    }

    #[test]
    fn insert_host_no_extension() {
        assert_eq!(
            insert_host_before_ext("clip", "www.youtube.com"),
            "clip.www.youtube.com"
        );
    }

    #[test]
    fn insert_host_trailing_dot() {
        assert_eq!(
            insert_host_before_ext("clip.", "www.youtube.com"),
            "clip..www.youtube.com"
        );
    }

    #[test]
    fn insert_host_dotfile_no_ext() {
        // Leading dot only ("hidden" style) — append rather than splitting on the leading dot.
        assert_eq!(
            insert_host_before_ext(".hidden", "vimeo.com"),
            ".hidden.vimeo.com"
        );
    }
}
