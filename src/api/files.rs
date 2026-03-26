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
        .route("/api/files/{id}/rate", post(rate))
        .route("/api/files/{id}", delete(remove))
}

#[derive(Serialize, FromRow)]
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
    created_at: Option<String>,
    rated_at: Option<String>,
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
) -> Result<Json<Vec<FileEntry>>, (StatusCode, Json<ErrorResponse>)> {
    let mut query = String::from(
        "SELECT id, queue_id, filename, original, path, rating, size_bytes, duration, thumbnail, category_id, created_at, rated_at FROM files",
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

    Ok(Json(entries))
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
        "SELECT id, queue_id, filename, original, path, rating, size_bytes, duration, thumbnail, category_id, created_at, rated_at FROM files WHERE id = ?"
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

    // Rename file on disk
    let download_dir = &state.config.storage.download_dir;
    let new_filename = rename::apply_rating(
        std::path::Path::new(download_dir),
        &file.filename,
        body.rating,
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Rename error: {}", e),
            }),
        )
    })?;

    let new_path = std::path::Path::new(download_dir)
        .join(&new_filename)
        .to_string_lossy()
        .to_string();

    // Update database
    let updated: FileEntry = sqlx::query_as(
        "UPDATE files SET filename = ?, path = ?, rating = ?, rated_at = CURRENT_TIMESTAMP WHERE id = ? RETURNING id, queue_id, filename, original, path, rating, size_bytes, duration, thumbnail, category_id, created_at, rated_at"
    )
    .bind(&new_filename)
    .bind(&new_path)
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
        "SELECT id, queue_id, filename, original, path, rating, size_bytes, duration, thumbnail, category_id, created_at, rated_at FROM files WHERE id = ?"
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
    let _ = std::fs::remove_file(&file.path);

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
