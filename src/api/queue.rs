use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/queue", post(create).get(list))
        .route("/api/queue/{id}", delete(cancel))
}

#[derive(Deserialize)]
struct CreateRequest {
    url: String,
    #[serde(default)]
    force: bool,
    category: Option<String>,
}

#[derive(Serialize, FromRow)]
struct QueueEntry {
    id: i64,
    url: String,
    status: String,
    progress: Option<String>,
    error: Option<String>,
    category_id: Option<i64>,
    created_at: Option<String>,
    updated_at: Option<String>,
}

#[derive(Deserialize)]
struct ListParams {
    status: Option<String>,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

fn is_valid_url(url: &str) -> bool {
    url::Url::parse(url)
        .map(|u| u.scheme() == "http" || u.scheme() == "https")
        .unwrap_or(false)
}

async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateRequest>,
) -> Result<(StatusCode, Json<QueueEntry>), (StatusCode, Json<ErrorResponse>)> {
    if !is_valid_url(&body.url) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse { error: "Invalid URL: must be a valid http or https URL".to_string() }),
        ));
    }

    // Check for duplicate URL unless force is set
    if !body.force {
        let existing: Option<QueueEntry> = sqlx::query_as(
            "SELECT id, url, status, progress, error, category_id, created_at, updated_at FROM queue WHERE url = ? LIMIT 1"
        )
        .bind(&body.url)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: format!("Database error: {}", e) }),
        ))?;

        if let Some(prev) = existing {
            return Err((
                StatusCode::CONFLICT,
                Json(ErrorResponse {
                    error: format!(
                        "URL already queued (id: {}, status: {}). Send with \"force\": true to re-download.",
                        prev.id, prev.status
                    ),
                }),
            ));
        }
    }

    // Resolve category if provided
    let category_id = if let Some(ref cat_name) = body.category {
        Some(
            super::categories::resolve_category(&state.db, &state.config.storage.download_dir, cat_name)
                .await
                .map_err(|e| (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse { error: e }),
                ))?
        )
    } else {
        None
    };

    let entry: QueueEntry = sqlx::query_as(
        "INSERT INTO queue (url, category_id) VALUES (?, ?) RETURNING id, url, status, progress, error, category_id, created_at, updated_at"
    )
    .bind(&body.url)
    .bind(category_id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse { error: format!("Database error: {}", e) }),
    ))?;

    Ok((StatusCode::CREATED, Json(entry)))
}

async fn list(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Result<Json<Vec<QueueEntry>>, (StatusCode, Json<ErrorResponse>)> {
    let entries: Vec<QueueEntry> = if let Some(status) = params.status {
        sqlx::query_as("SELECT id, url, status, progress, error, category_id, created_at, updated_at FROM queue WHERE status = ? ORDER BY created_at DESC")
            .bind(status)
            .fetch_all(&state.db)
            .await
    } else {
        sqlx::query_as("SELECT id, url, status, progress, error, category_id, created_at, updated_at FROM queue ORDER BY created_at DESC")
            .fetch_all(&state.db)
            .await
    }
    .map_err(|e| (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse { error: format!("Database error: {}", e) }),
    ))?;

    Ok(Json(entries))
}

async fn cancel(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let result = sqlx::query("DELETE FROM queue WHERE id = ? AND status = 'pending'")
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(|e| (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: format!("Database error: {}", e) }),
        ))?;

    if result.rows_affected() == 0 {
        Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse { error: "Queue entry not found or not in pending state".to_string() }),
        ))
    } else {
        Ok(StatusCode::NO_CONTENT)
    }
}
