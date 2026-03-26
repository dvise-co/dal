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
        .route("/api/queue/{id}", delete(cancel))
        .route("/api/queue/{id}/archive", post(archive))
        .route("/api/queue/{id}/unarchive", post(unarchive))
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
    created_at: Option<String>,
    updated_at: Option<String>,
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
            "SELECT id, url, status, progress, error, output, category_id, position, archived, created_at, updated_at FROM queue WHERE url = ? LIMIT 1"
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
        "INSERT INTO queue (url, category_id, position) VALUES (?, ?, ?) RETURNING id, url, status, progress, error, output, category_id, position, archived, created_at, updated_at"
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
        "SELECT id, url, status, progress, error, output, category_id, position, archived, created_at, updated_at FROM queue",
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
