use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

pub async fn init_db(database_path: &str) -> Result<SqlitePool, Box<dyn std::error::Error>> {
    // Ensure parent directory exists
    if let Some(parent) = std::path::Path::new(database_path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    let options = SqliteConnectOptions::from_str(database_path)?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    tracing::info!("Database initialized at {}", database_path);
    Ok(pool)
}
