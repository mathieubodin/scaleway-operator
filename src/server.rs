use axum::{extract::State, routing::get, Router};
use axum::http::StatusCode;
use prometheus::{Encoder, TextEncoder};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::context::Context;

#[derive(Clone)]
pub struct AppState {
    pub ctx: Arc<Context>,
    pub registry: Arc<prometheus::Registry>,
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics_handler))
        .route("/log-level", get(log_level))
        .with_state(state)
}

pub async fn run_axum_server(ctx: Arc<Context>, registry: Arc<prometheus::Registry>) {
    let app = build_router(AppState { ctx, registry });

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .expect("failed to bind axum server to :8080");

    tracing::info!("Axum server listening on :8080");

    axum::serve(listener, app)
        .await
        .expect("axum server error");
}

async fn healthz() -> (StatusCode, &'static str) {
    (StatusCode::OK, "ok")
}

async fn readyz(State(state): State<AppState>) -> StatusCode {
    let last = state.ctx.last_reconcile_at.load(Ordering::Acquire);
    if last == 0 {
        return StatusCode::SERVICE_UNAVAILABLE;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    if now - last < 60 {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

async fn metrics_handler(
    State(state): State<AppState>,
) -> (StatusCode, [(axum::http::HeaderName, &'static str); 1], String) {
    let encoder = TextEncoder::new();
    let metric_families = state.registry.gather();
    let mut buf = Vec::new();
    if let Err(e) = encoder.encode(&metric_families, &mut buf) {
        tracing::error!(error = %e, "Failed to encode metrics");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(axum::http::header::CONTENT_TYPE, "text/plain")],
            String::new(),
        );
    }
    let body = String::from_utf8(buf).unwrap_or_default();
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        body,
    )
}

async fn log_level() -> (StatusCode, String) {
    let level = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    (StatusCode::OK, level)
}
