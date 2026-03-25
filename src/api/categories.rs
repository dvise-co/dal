use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/categories", get(list).post(create))
        .route("/api/categories/{id}", put(rename).delete(remove))
}

#[derive(Serialize, FromRow, Clone)]
pub struct Category {
    pub id: i64,
    pub name: String,
    pub created_at: Option<String>,
}

#[derive(Deserialize)]
struct CreateRequest {
    name: String,
}

#[derive(Deserialize)]
struct RenameRequest {
    name: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

fn is_valid_category_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && name.chars().all(|c| c.is_alphanumeric() || c == ' ' || c == '-' || c == '_')
        && name != "uncategorized"
}

async fn list(
    State(state): State<AppState>,
) -> Result<Json<Vec<Category>>, (StatusCode, Json<ErrorResponse>)> {
    let categories: Vec<Category> = sqlx::query_as(
        "SELECT id, name, created_at FROM categories ORDER BY name ASC",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse { error: format!("Database error: {}", e) }),
    ))?;

    Ok(Json(categories))
}

async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateRequest>,
) -> Result<(StatusCode, Json<Category>), (StatusCode, Json<ErrorResponse>)> {
    let name = body.name.trim().to_string();

    if !is_valid_category_name(&name) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid category name: use only letters, numbers, spaces, hyphens, and underscores".to_string(),
            }),
        ));
    }

    let category: Category = sqlx::query_as(
        "INSERT INTO categories (name) VALUES (?) RETURNING id, name, created_at",
    )
    .bind(&name)
    .fetch_one(&state.db)
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            (
                StatusCode::CONFLICT,
                Json(ErrorResponse { error: format!("Category '{}' already exists", name) }),
            )
        } else {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: format!("Database error: {}", e) }),
            )
        }
    })?;

    // Create the directory on disk
    let dir = std::path::Path::new(&state.config.storage.download_dir).join(&name);
    let _ = std::fs::create_dir_all(&dir);

    Ok((StatusCode::CREATED, Json(category)))
}

async fn rename(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<RenameRequest>,
) -> Result<Json<Category>, (StatusCode, Json<ErrorResponse>)> {
    let new_name = body.name.trim().to_string();

    if !is_valid_category_name(&new_name) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid category name: use only letters, numbers, spaces, hyphens, and underscores".to_string(),
            }),
        ));
    }

    // Get current category
    let old: Category = sqlx::query_as(
        "SELECT id, name, created_at FROM categories WHERE id = ?",
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
        Json(ErrorResponse { error: "Category not found".to_string() }),
    ))?;

    // Rename folder on disk
    let download_dir = &state.config.storage.download_dir;
    let old_dir = std::path::Path::new(download_dir).join(&old.name);
    let new_dir = std::path::Path::new(download_dir).join(&new_name);

    if old_dir.exists() {
        std::fs::rename(&old_dir, &new_dir).map_err(|e| (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: format!("Failed to rename folder: {}", e) }),
        ))?;
    }

    // Update category name in DB
    let updated: Category = sqlx::query_as(
        "UPDATE categories SET name = ? WHERE id = ? RETURNING id, name, created_at",
    )
    .bind(&new_name)
    .bind(id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse { error: format!("Database error: {}", e) }),
    ))?;

    // Update file paths in DB
    let old_prefix = format!("{}/", old.name);
    let new_prefix = format!("{}/", new_name);
    let _ = sqlx::query(
        "UPDATE files SET path = REPLACE(path, ?, ?) WHERE category_id = ?",
    )
    .bind(&old_prefix)
    .bind(&new_prefix)
    .bind(id)
    .execute(&state.db)
    .await;

    Ok(Json(updated))
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    // Get category
    let category: Category = sqlx::query_as(
        "SELECT id, name, created_at FROM categories WHERE id = ?",
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
        Json(ErrorResponse { error: "Category not found".to_string() }),
    ))?;

    let download_dir = &state.config.storage.download_dir;
    let cat_dir = std::path::Path::new(download_dir).join(&category.name);
    let uncat_dir = std::path::Path::new(download_dir).join("uncategorized");

    // Move files to uncategorized
    if cat_dir.exists() {
        let _ = std::fs::create_dir_all(&uncat_dir);
        if let Ok(entries) = std::fs::read_dir(&cat_dir) {
            for entry in entries.flatten() {
                let dest = uncat_dir.join(entry.file_name());
                let _ = std::fs::rename(entry.path(), &dest);
            }
        }
        let _ = std::fs::remove_dir_all(&cat_dir);
    }

    // Update DB: move files to uncategorized (NULL category_id)
    let old_prefix = format!("{}/{}/", download_dir, category.name);
    let new_prefix = format!("{}/uncategorized/", download_dir);
    let _ = sqlx::query(
        "UPDATE files SET category_id = NULL, path = REPLACE(path, ?, ?) WHERE category_id = ?",
    )
    .bind(&old_prefix)
    .bind(&new_prefix)
    .bind(id)
    .execute(&state.db)
    .await;

    let _ = sqlx::query("UPDATE queue SET category_id = NULL WHERE category_id = ?")
        .bind(id)
        .execute(&state.db)
        .await;

    // Delete category
    sqlx::query("DELETE FROM categories WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(|e| (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: format!("Database error: {}", e) }),
        ))?;

    Ok(StatusCode::NO_CONTENT)
}

/// Resolve a category name to an ID, creating it if it doesn't exist.
pub async fn resolve_category(
    db: &sqlx::SqlitePool,
    download_dir: &str,
    name: &str,
) -> Result<i64, String> {
    let name = name.trim();
    if !is_valid_category_name(name) {
        return Err("Invalid category name".to_string());
    }

    // Try to find existing
    if let Ok(Some(cat)) = sqlx::query_as::<_, Category>(
        "SELECT id, name, created_at FROM categories WHERE name = ?",
    )
    .bind(name)
    .fetch_optional(db)
    .await
    {
        return Ok(cat.id);
    }

    // Create new
    let cat: Category = sqlx::query_as(
        "INSERT INTO categories (name) VALUES (?) RETURNING id, name, created_at",
    )
    .bind(name)
    .fetch_one(db)
    .await
    .map_err(|e| format!("Failed to create category: {}", e))?;

    let dir = std::path::Path::new(download_dir).join(name);
    let _ = std::fs::create_dir_all(&dir);

    Ok(cat.id)
}
