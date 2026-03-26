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
