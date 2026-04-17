use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;

use crate::handlers::generate_handler;
use crate::AppState;

pub fn create_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/health", get(health_check))
        .route("/generate", post(generate_handler))
}

async fn health_check() -> &'static str {
    "AetherOS operational"
}
