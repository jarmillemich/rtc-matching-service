use axum::{
    extract::{
        ws::{WebSocket, WebSocketUpgrade},
        State,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use axum_macros::debug_handler;
use std::sync::Arc;
use tower_http::services::ServeDir;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod matching_service;
use matching_service::MatchingService;
mod types;
use types::*;

#[tokio::main]
async fn main() {
    // Set up logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "rtc_matching_service=trace,tower_http=debug,axum::rejection=trace".into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::debug!("Starting server");

    let matcher = Arc::new(MatchingService::default());

    let app = Router::new()
        .fallback_service(ServeDir::new("./static/").append_index_html_on_directories(true))
        // Just determines that the server is alive
        .route("/ping", get(|| async { "pong" }))
        // Registers a new session and attaches a websocket
        // to it so we can send connection requests back
        .route("/host", get(host_session_handler))
        // Returns a list of all available session names
        .route("/list", get(get_session_list))
        // Attempts to join a particular session
        // Accepts and RTC offer and returns the offer from the server
        .route("/join", post(join_session_handler))
        .with_state(matcher);

    axum::Server::bind(&"127.0.0.1:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();
}

#[debug_handler]
async fn host_session_handler(
    State(db): State<Arc<MatchingService>>,
    ws: WebSocketUpgrade,
    //Json(parameters): Json<StartHosting>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| socket_handler(db, socket))
}

async fn socket_handler(db: Arc<MatchingService>, socket: WebSocket) {
    match socket_handler_inner(db, socket).await {
        Ok(_) => (),
        Err(e) => {
            // TODO we can't return an error from this?
            tracing::error!("Error in host session: {}", e);
        }
    }
}

async fn socket_handler_inner(
    db: Arc<MatchingService>,
    mut socket: WebSocket,
) -> anyhow::Result<()> {
    // The host must send us the initiation message
    let initial_message = socket
        .recv()
        .await
        .ok_or_else(|| anyhow::anyhow!("Did not correctly receive session init message"))?
        .map_err(|_| anyhow::anyhow!("Failed to receive session init message"))?
        .into_text()
        .map_err(|_| anyhow::anyhow!("Failed to parse session init message"))?;

    let parameters: StartHosting = serde_json::from_str(&initial_message)
        .map_err(|e| anyhow::anyhow!("Failed to parse session init message: {}", e.to_string()))?;

    println!("Starting host session: {}", parameters.name);

    db.init_host_session(socket, parameters.name.clone())
        .await
        .unwrap();

    Ok(())
}

#[debug_handler]
async fn get_session_list(
    State(db): State<Arc<MatchingService>>,
) -> (StatusCode, Json<Vec<String>>) {
    let sessions = db.list_sessions().await.unwrap();
    (StatusCode::OK, Json(sessions))
}

#[debug_handler]
async fn join_session_handler(
    State(db): State<Arc<MatchingService>>,
    Json(join_session): Json<JoinSessionRequest>,
) -> (StatusCode, Json<Option<JoinSessionResponse>>) {
    let server_response = db.attempt_join(join_session).await;

    match server_response {
        Ok(server_response) => (StatusCode::OK, Json(Some(server_response))),
        Err(e) => {
            tracing::error!("Error in join session: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(None))
        }
    }
}
