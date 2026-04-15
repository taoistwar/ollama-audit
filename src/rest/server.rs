use crate::AppState;
use crate::get_bind_addr;
use crate::get_proxy_handler;
use crate::init_tracing;
use crate::post_proxy_handler;
use axum::{Router, routing::get};

use tokio::net::TcpListener;
use tracing::info;

pub async fn start_server() {
    dotenvy::dotenv().ok();

    let _log_guard = init_tracing();

    let state = AppState::factory();
    let app = Router::new()
        .route("/{*path}", get(get_proxy_handler).post(post_proxy_handler))
        .with_state(state);

    let addr = get_bind_addr();
    info!("ollama-audit running at http://{}", addr);
    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
