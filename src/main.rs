pub mod api;
pub mod brain;
pub mod config;
pub mod core;
pub mod error;
pub mod handlers;
pub mod middleware;

use axum::middleware as axum_middleware;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::brain::BrainClient;
use crate::config::AppConfig;
use crate::core::bloom::BloomFilter;
use crate::core::cache::SemanticCache;
use crate::core::qdrant::VectorDB;
use crate::core::rate_limit::RateLimiter;
use crate::core::vector::VectorEngine;
use crate::middleware::shield::rate_limit_middleware;

#[derive(Clone)]
pub struct AppState {
    pub rate_limiter: Arc<RateLimiter>,
    pub bloom_filter: Arc<BloomFilter>,
    pub vector_engine: Arc<VectorEngine>,
    pub semantic_cache: Arc<SemanticCache>,
    pub vector_db: Arc<VectorDB>,
    pub config: Arc<AppConfig>,
    pub brain_client: BrainClient,
}

#[tokio::main]
async fn main() {
    init_tracing();

    let config = match load_config() {
        Ok(c) => Arc::new(c),
        Err(e) => {
            eprintln!("FATAL: Configuration error: {}", e);
            std::process::exit(1);
        }
    };

    info!(
        port = config.port,
        rate_limit_rps = config.rate_limit_rps,
        bloom_capacity = config.bloom_capacity,
        "aether_os_starting"
    );

    let state = match build_state(Arc::clone(&config)).await {
        Ok(s) => s,
        Err(e) => {
            error!(error = %e, "state_initialization_failed");
            std::process::exit(1);
        }
    };

    // FIX: Wrap the entire state in an Arc before passing it to the router!
    let shared_state = Arc::new(state);
    let app = build_router(shared_state);

    let addr: SocketAddr = format!("0.0.0.0:{}", config.port)
        .parse()
        .expect("Invalid socket address");

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => {
            info!(address = %addr, "socket_bound");
            l
        }
        Err(e) => {
            error!(address = %addr, error = %e, "socket_bind_failed");
            std::process::exit(1);
        }
    };

    info!(address = %addr, "gateway_online");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .unwrap_or_else(|e| {
        error!(error = %e, "server_crashed");
        std::process::exit(1);
    });

    info!("gateway_shutdown_complete");
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "aether_os=debug,tower_http=debug,warn".into()),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_thread_ids(false)
                .with_file(false),
        )
        .init();
}

fn load_config() -> Result<AppConfig, String> {
    Ok(AppConfig::from_env())
}

async fn build_state(config: Arc<AppConfig>) -> anyhow::Result<AppState> {
    let rate_limiter = Arc::new(RateLimiter::new(
        config.rate_limit_capacity as f64,
        config.rate_limit_rps as f64,
    ));

    let bloom_filter = Arc::new(BloomFilter::new(
        config.bloom_capacity as usize,
        config.bloom_fp_rate as f64,
    ));

    info!(
        bloom_memory_kb = bloom_filter.memory_bytes() / 1024,
        "bloom_filter_initialized"
    );

    info!("vector_engine_loading — this takes 2–5 seconds on first run");

    let vector_engine = tokio::task::spawn_blocking(|| VectorEngine::new())
        .await
        .map_err(|e| anyhow::anyhow!("spawn_blocking panicked: {:?}", e))?
        .map_err(|e| {
            error!(error = %e, "vector_engine_init_failed");
            e
        })?;

    let vector_engine = Arc::new(vector_engine);
    info!("vector_engine_ready");

    let redis_url =
        std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let semantic_cache = Arc::new(SemanticCache::new(&redis_url).await);

    // --- NEW HTTP BRAIN CONNECTION ---
    let brain_url =
        std::env::var("BRAIN_URL").unwrap_or_else(|_| "http://localhost:8000".to_string());
    info!(url = %brain_url, "brain_client_initializing");

    let brain_client = BrainClient::new(brain_url)
        .map_err(|e| anyhow::anyhow!("Failed to create brain client: {}", e))?;

    // Keep-Alive Heartbeat to prevent Render Free Tier sleep
    let keepalive_client = brain_client.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(14 * 60));
        loop {
            interval.tick().await;
            let _ = keepalive_client.health_check().await;
            tracing::debug!("brain_keepalive_ping_sent");
        }
    });

    info!("brain_client_connected");

    let qdrant_url =
        std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6334".to_string());

    let qdrant_collection =
        std::env::var("QDRANT_COLLECTION").unwrap_or_else(|_| "aetheros_knowledge".to_string());

    let vector_db = Arc::new(
        VectorDB::new(&qdrant_url, &qdrant_collection)
            .await
            .map_err(|e| {
                error!(error = %e, "qdrant_init_failed");
                e
            })?,
    );

    Ok(AppState {
        rate_limiter,
        bloom_filter,
        vector_engine,
        semantic_cache,
        vector_db,
        config,
        brain_client,
    })
}

fn build_router(state: Arc<AppState>) -> axum::Router {
    use axum::http::header;
    use axum::http::HeaderValue;
    use axum::http::Method;
    use tower_http::cors::CorsLayer;
    use tower_http::trace::TraceLayer;

    let rate_limiter = Arc::clone(&state.rate_limiter);

    let frontend_url =
        std::env::var("FRONTEND_URL").unwrap_or_else(|_| "http://localhost:3001".to_string());

    let origin = frontend_url
        .parse::<HeaderValue>()
        .expect("Invalid FRONTEND_URL");

    let cors = CorsLayer::new()
        .allow_origin(origin)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION, header::ACCEPT])
        .allow_credentials(true);

    api::routes::create_router()
        //.layer(axum_middleware::from_fn_with_state(
        //    rate_limiter,
        //  rate_limit_middleware,
        //))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install CTRL+C handler");
    };

    #[cfg(unix)]
    let sigterm = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let sigterm = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c  => { info!(signal = "SIGINT",  "shutdown_signal_received") }
        _ = sigterm => { info!(signal = "SIGTERM", "shutdown_signal_received") }
    }

    info!("initiating_graceful_shutdown");
}
