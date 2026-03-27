use crate::AppState;
use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;

pub mod categories;
pub mod files;
pub mod queue;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(queue::router())
        .merge(files::router())
        .merge(categories::router())
        .route("/api/health", axum::routing::get(health))
        .route("/api/ws", axum::routing::get(ws_handler))
}

async fn health() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({ "status": "ok" }))
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(mut socket: WebSocket, state: AppState) {
    let mut rx = state.progress_tx.subscribe();

    while let Ok(update) = rx.recv().await {
        let msg = serde_json::to_string(&update).unwrap_or_default();
        if socket.send(Message::text(msg)).await.is_err() {
            break;
        }
    }
}
