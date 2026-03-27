use crate::AppState;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, post, put},
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/queue", post(create).get(list))
        .route("/api/queue/reorder", put(reorder))
        .route("/api/queue/archive-all", post(archive_all))
        .route("/api/queue/retry-all", post(retry_all))
        .route("/api/queue/{id}", delete(cancel))
        .route("/api/queue/{id}/archive", post(archive))
        .route("/api/queue/{id}/unarchive", post(unarchive))
        .route("/api/queue/{id}/retry", post(retry))
        .route("/api/queue/{id}/move-next", post(move_next))
        .route("/api/queue/{id}/history", axum::routing::get(history))
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
    output: Option<String>,
    category_id: Option<i64>,
    position: Option<i64>,
    archived: Option<bool>,
    attempts: Option<i64>,
    created_at: Option<String>,
    updated_at: Option<String>,
}

#[derive(Serialize, FromRow)]
struct HistoryEntry {
    id: i64,
    queue_id: i64,
    status: String,
    error: Option<String>,
    output: Option<String>,
    created_at: Option<String>,
}

#[derive(Deserialize)]
struct ReorderRequest {
    order: Vec<i64>,
}

#[derive(Deserialize)]
struct ArchiveAllRequest {
    status: Option<String>,
}

#[derive(Deserialize)]
struct ListParams {
    status: Option<String>,
    archived: Option<bool>,
    include_archived: Option<bool>,
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
            Json(ErrorResponse {
                error: "Invalid URL: must be a valid http or https URL".to_string(),
            }),
        ));
    }

    // Check for duplicate URL unless force is set
    if !body.force {
        let existing: Option<QueueEntry> = sqlx::query_as(
            "SELECT id, url, status, progress, error, output, category_id, position, archived, attempts, created_at, updated_at FROM queue WHERE url = ? LIMIT 1"
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
            super::categories::resolve_category(
                &state.db,
                &state.config.storage.download_dir,
                cat_name,
            )
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })))?,
        )
    } else {
        None
    };

    // Assign position at end of queue
    let max_pos: Option<i64> = sqlx::query_scalar("SELECT MAX(position) FROM queue")
        .fetch_one(&state.db)
        .await
        .unwrap_or(None);
    let next_pos = max_pos.unwrap_or(-1) + 1;

    let entry: QueueEntry = sqlx::query_as(
        "INSERT INTO queue (url, category_id, position) VALUES (?, ?, ?) RETURNING id, url, status, progress, error, output, category_id, position, archived, attempts, created_at, updated_at"
    )
    .bind(&body.url)
    .bind(category_id)
    .bind(next_pos)
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
    let mut query = String::from(
        "SELECT id, url, status, progress, error, output, category_id, position, archived, attempts, created_at, updated_at FROM queue",
    );
    let mut conditions: Vec<String> = Vec::new();

    if let Some(ref status) = params.status {
        conditions.push(format!("status = '{}'", status.replace('\'', "")));
    }

    if params.archived == Some(true) {
        conditions.push("archived = TRUE".to_string());
    } else if params.include_archived != Some(true) {
        conditions.push("(archived = FALSE OR archived IS NULL)".to_string());
    }

    if !conditions.is_empty() {
        query.push_str(" WHERE ");
        query.push_str(&conditions.join(" AND "));
    }

    query.push_str(" ORDER BY position ASC, created_at DESC");

    let entries: Vec<QueueEntry> =
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

async fn cancel(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let result = sqlx::query("DELETE FROM queue WHERE id = ? AND status = 'pending'")
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

    if result.rows_affected() == 0 {
        Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Queue entry not found or not in pending state".to_string(),
            }),
        ))
    } else {
        Ok(StatusCode::NO_CONTENT)
    }
}

async fn reorder(
    State(state): State<AppState>,
    Json(body): Json<ReorderRequest>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    for (pos, id) in body.order.iter().enumerate() {
        sqlx::query("UPDATE queue SET position = ? WHERE id = ?")
            .bind(pos as i64)
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
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn move_next(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    // Get the first pending entry (the one currently "next up")
    let first_pending: Option<(i64, i64)> = sqlx::query_as(
        "SELECT id, position FROM queue WHERE status = 'pending' AND (archived = FALSE OR archived IS NULL) ORDER BY position ASC LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: format!("Database error: {}", e) }),
        )
    })?;

    // Get the target entry
    let target: Option<(i64, i64)> =
        sqlx::query_as("SELECT id, position FROM queue WHERE id = ? AND status = 'pending'")
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
            })?;

    let (target_id, _target_pos) = target.ok_or((
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "Entry not found or not in pending state".to_string(),
        }),
    ))?;

    // If there's no other pending entry or the target is already first, nothing to do
    let (_first_id, first_pos) = match first_pending {
        Some(f) if f.0 != target_id => f,
        _ => return Ok(StatusCode::NO_CONTENT),
    };

    // Place the target right before the current first pending entry
    let new_pos = first_pos - 1;
    sqlx::query("UPDATE queue SET position = ? WHERE id = ?")
        .bind(new_pos)
        .bind(target_id)
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

async fn archive(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    sqlx::query("UPDATE queue SET archived = TRUE WHERE id = ?")
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

async fn unarchive(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    sqlx::query("UPDATE queue SET archived = FALSE WHERE id = ?")
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

async fn archive_all(
    State(state): State<AppState>,
    Json(body): Json<ArchiveAllRequest>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let query = if let Some(ref status) = body.status {
        format!(
            "UPDATE queue SET archived = TRUE WHERE status = '{}' AND (archived = FALSE OR archived IS NULL)",
            status.replace('\'', "")
        )
    } else {
        "UPDATE queue SET archived = TRUE WHERE status IN ('completed', 'failed') AND (archived = FALSE OR archived IS NULL)".to_string()
    };
    sqlx::query(&query).execute(&state.db).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Database error: {}", e),
            }),
        )
    })?;
    Ok(StatusCode::NO_CONTENT)
}

async fn retry(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    // Record history before resetting
    let _ = sqlx::query(
        "INSERT INTO queue_history (queue_id, status, error, output) SELECT id, status, error, output FROM queue WHERE id = ?",
    )
    .bind(id)
    .execute(&state.db)
    .await;

    let result = sqlx::query(
        "UPDATE queue SET status = 'pending', progress = NULL, error = NULL, output = NULL, archived = FALSE, updated_at = CURRENT_TIMESTAMP WHERE id = ? AND status IN ('failed', 'completed')",
    )
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

    if result.rows_affected() == 0 {
        Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Entry not found or not in a retryable state".to_string(),
            }),
        ))
    } else {
        Ok(StatusCode::NO_CONTENT)
    }
}

async fn retry_all(
    State(state): State<AppState>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    // Record history for all failed entries
    let _ = sqlx::query(
        "INSERT INTO queue_history (queue_id, status, error, output) SELECT id, status, error, output FROM queue WHERE status = 'failed'",
    )
    .execute(&state.db)
    .await;

    sqlx::query(
        "UPDATE queue SET status = 'pending', progress = NULL, error = NULL, output = NULL, archived = FALSE, updated_at = CURRENT_TIMESTAMP WHERE status = 'failed'",
    )
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

async fn history(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<HistoryEntry>>, (StatusCode, Json<ErrorResponse>)> {
    let entries: Vec<HistoryEntry> = sqlx::query_as(
        "SELECT id, queue_id, status, error, output, created_at FROM queue_history WHERE queue_id = ? ORDER BY created_at DESC",
    )
    .bind(id)
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
