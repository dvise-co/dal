use crate::AppState;
use crate::rename;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/files", get(list))
        .route("/api/files/scan", post(scan))
        .route("/api/files/{id}/rate", post(rate))
        .route("/api/files/{id}/requeue", post(requeue))
        .route("/api/files/{id}", delete(remove))
}

#[derive(Serialize, FromRow, Clone)]
struct FileEntry {
    id: i64,
    queue_id: Option<i64>,
    filename: String,
    original: String,
    path: String,
    rating: Option<i32>,
    size_bytes: Option<i64>,
    duration: Option<String>,
    thumbnail: Option<String>,
    category_id: Option<i64>,
    provisional: Option<bool>,
    created_at: Option<String>,
    rated_at: Option<String>,
}

#[derive(Serialize)]
struct FileResponse {
    #[serde(flatten)]
    entry: FileEntry,
    display_path: String,
    exists: bool,
}

#[derive(Deserialize)]
struct ListParams {
    unrated: Option<bool>,
    incomplete: Option<bool>,
    category_id: Option<i64>,
    sort: Option<String>,
}

#[derive(Deserialize)]
struct RateRequest {
    rating: u32,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

async fn list(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Result<Json<Vec<FileResponse>>, (StatusCode, Json<ErrorResponse>)> {
    let mut query = String::from(
        "SELECT id, queue_id, filename, original, path, rating, size_bytes, duration, thumbnail, category_id, provisional, created_at, rated_at FROM files",
    );
    let mut conditions: Vec<String> = Vec::new();

    if params.unrated == Some(true) {
        conditions.push("rating IS NULL".to_string());
    }
    if params.incomplete == Some(true) {
        conditions.push("filename LIKE 'INCOMPLETE - %'".to_string());
    }
    if let Some(cat_id) = params.category_id {
        conditions.push(format!("category_id = {}", cat_id));
    }

    if !conditions.is_empty() {
        query.push_str(" WHERE ");
        query.push_str(&conditions.join(" AND "));
    }

    match params.sort.as_deref() {
        Some("rating") => query.push_str(" ORDER BY rating DESC NULLS LAST"),
        Some("name") => query.push_str(" ORDER BY filename ASC"),
        _ => query.push_str(" ORDER BY created_at DESC"),
    }

    let entries: Vec<FileEntry> =
        sqlx::query_as(&query)
            .fetch_all(&state.db)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("Database error: {}", e),
                    }),
                )
            })?;

    let storage = &state.config.storage;
    let responses: Vec<FileResponse> = entries
        .into_iter()
        .map(|e| {
            let full = storage.full_path(&e.path);
            let exists = std::path::Path::new(&full).exists();
            let display_path = storage.display_path(&e.path);
            FileResponse {
                entry: e,
                display_path,
                exists,
            }
        })
        .collect();

    Ok(Json(responses))
}

async fn rate(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<RateRequest>,
) -> Result<Json<FileEntry>, (StatusCode, Json<ErrorResponse>)> {
    if !(10..=99).contains(&body.rating) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Rating must be between 10 and 99".to_string(),
            }),
        ));
    }

    // Get current file record
    let file: FileEntry = sqlx::query_as(
        "SELECT id, queue_id, filename, original, path, rating, size_bytes, duration, thumbnail, category_id, provisional, created_at, rated_at FROM files WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse { error: format!("Database error: {}", e) }),
    ))?
    .ok_or_else(|| (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse { error: "File not found".to_string() }),
    ))?;

    // Resolve full filesystem path for disk rename
    let full_path = state.config.storage.full_path(&file.path);
    let file_dir = std::path::Path::new(&full_path)
        .parent()
        .unwrap_or(std::path::Path::new(&state.config.storage.download_dir));
    let new_filename =
        rename::apply_rating(file_dir, &file.filename, body.rating).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Rename error: {}", e),
                }),
            )
        })?;

    // Store relative path in DB
    let new_full = file_dir.join(&new_filename).to_string_lossy().to_string();
    let new_relative = state.config.storage.to_relative_path(&new_full);

    // Update database
    let updated: FileEntry = sqlx::query_as(
        "UPDATE files SET filename = ?, path = ?, rating = ?, rated_at = CURRENT_TIMESTAMP WHERE id = ? RETURNING id, queue_id, filename, original, path, rating, size_bytes, duration, thumbnail, category_id, provisional, created_at, rated_at"
    )
    .bind(&new_filename)
    .bind(&new_relative)
    .bind(body.rating as i32)
    .bind(id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse { error: format!("Database error: {}", e) }),
    ))?;

    Ok(Json(updated))
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    // Get file record
    let file: FileEntry = sqlx::query_as(
        "SELECT id, queue_id, filename, original, path, rating, size_bytes, duration, thumbnail, category_id, provisional, created_at, rated_at FROM files WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse { error: format!("Database error: {}", e) }),
    ))?
    .ok_or_else(|| (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse { error: "File not found".to_string() }),
    ))?;

    // Delete file from disk (ignore if already missing)
    let full_path = state.config.storage.full_path(&file.path);
    let _ = std::fs::remove_file(&full_path);

    // Delete from database
    sqlx::query("DELETE FROM files WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?;

    Ok(StatusCode::NO_CONTENT)
}

/// Requeue a provisional file — reset the linked queue entry to pending and delete the file.
async fn requeue(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let file: FileEntry = sqlx::query_as(
        "SELECT id, queue_id, filename, original, path, rating, size_bytes, duration, thumbnail, category_id, provisional, created_at, rated_at FROM files WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Database error: {}", e),
            }),
        )
    })?
    .ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "File not found".to_string(),
            }),
        )
    })?;

    // Delete the file from disk
    let full_path = state.config.storage.full_path(&file.path);
    let _ = std::fs::remove_file(&full_path);

    // Delete the file record
    let _ = sqlx::query("DELETE FROM files WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await;

    // Reset the linked queue entry to pending
    if let Some(queue_id) = file.queue_id {
        // Record history
        let _ = sqlx::query(
            "INSERT INTO queue_history (queue_id, status, error) VALUES (?, 'requeued', 'Requeued from review — provisional file deleted')",
        )
        .bind(queue_id)
        .execute(&state.db)
        .await;

        let _ = sqlx::query(
            "UPDATE queue SET status = 'pending', progress = NULL, error = NULL, output = NULL, archived = FALSE, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        )
        .bind(queue_id)
        .execute(&state.db)
        .await;
    }

    Ok(StatusCode::NO_CONTENT)
}

/// Scan the download directory for files not tracked in the database.
/// Creates DB records for any untracked files found.
async fn scan(
    State(state): State<AppState>,
) -> Result<Json<Vec<FileResponse>>, (StatusCode, Json<ErrorResponse>)> {
    let download_dir = &state.config.storage.download_dir;
    let mut new_files = Vec::new();

    // Get all known paths from DB
    let known_paths: Vec<String> = sqlx::query_scalar("SELECT path FROM files")
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

    let known_set: std::collections::HashSet<String> = known_paths.into_iter().collect();

    // Walk the download directory recursively
    fn walk_dir(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk_dir(&path, files);
                } else if path.is_file() {
                    // Skip .part files (incomplete yt-dlp downloads)
                    if let Some(ext) = path.extension()
                        && ext == "part"
                    {
                        continue;
                    }
                    files.push(path);
                }
            }
        }
    }

    let mut disk_files = Vec::new();
    walk_dir(std::path::Path::new(download_dir), &mut disk_files);

    let storage = &state.config.storage;

    for path in disk_files {
        let path_str = path.to_string_lossy().to_string();
        let relative = storage.to_relative_path(&path_str);
        if known_set.contains(&relative) {
            continue;
        }

        let filename = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let size_bytes = std::fs::metadata(&path).map(|m| m.len() as i64).ok();

        // Parse existing rating from filename if present
        let rating = crate::rename::parse_rating(&filename).map(|r| r as i32);

        let entry: FileEntry = sqlx::query_as(
            "INSERT INTO files (filename, original, path, size_bytes, rating) VALUES (?, ?, ?, ?, ?) RETURNING id, queue_id, filename, original, path, rating, size_bytes, duration, thumbnail, category_id, provisional, created_at, rated_at"
        )
        .bind(&filename)
        .bind(&filename)
        .bind(&relative)
        .bind(size_bytes)
        .bind(rating)
        .fetch_one(&state.db)
        .await
        .map_err(|e| (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: format!("Database error: {}", e) }),
        ))?;

        let display_path = storage.display_path(&entry.path);
        new_files.push(FileResponse {
            entry,
            display_path,
            exists: true,
        });
    }

    Ok(Json(new_files))
}
