/// Unit tests for the axum HTTP server handlers (U4).
///
/// Uses `tower::ServiceExt::oneshot` to call handlers without a live TCP listener.
use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    routing::get,
    Router,
};
use http_body_util::BodyExt;
use prometheus::{Encoder, TextEncoder};
use scaleway_operator::{
    context::Context,
    metrics::OperatorMetrics,
    scaleway::ScalewayClient,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tower::ServiceExt;

// ── AppState (mirrors main.rs) ────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    ctx: Arc<Context>,
    registry: Arc<prometheus::Registry>,
}

// ── Handler implementations (mirrors main.rs) ────────────────────────────────

async fn healthz() -> (StatusCode, &'static str) {
    (StatusCode::OK, "ok")
}

async fn readyz(State(state): State<AppState>) -> StatusCode {
    let last = state.ctx.last_reconcile_at.load(Ordering::Relaxed);
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
) -> (
    StatusCode,
    [(axum::http::HeaderName, &'static str); 1],
    String,
) {
    let encoder = TextEncoder::new();
    let metric_families = state.registry.gather();
    let mut buf = Vec::new();
    if encoder.encode(&metric_families, &mut buf).is_err() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(axum::http::header::CONTENT_TYPE, "text/plain")],
            String::new(),
        );
    }
    let body = String::from_utf8_lossy(&buf).into_owned();
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

// ── Helpers ───────────────────────────────────────────────────────────────────

fn build_registry_and_metrics() -> (Arc<prometheus::Registry>, OperatorMetrics) {
    let registry = prometheus::Registry::new();
    let metrics = OperatorMetrics::new(&registry).expect("metrics registration failed");
    (Arc::new(registry), metrics)
}

fn build_context_with_metrics(
    metrics: OperatorMetrics,
    last_reconcile_at: i64,
) -> Arc<Context> {
    Arc::new(Context {
        client: {
            // Use a minimal kube client config that won't connect to any cluster.
            // Tests that use the context only read last_reconcile_at or metrics.
            let config = kube::Config::new("http://localhost:0".parse().unwrap());
            kube::Client::try_from(config).expect("failed to build test kube client")
        },
        scaleway_client: ScalewayClient::new_with_base_url(
            "test-token".to_string(),
            "http://localhost:0".to_string(),
        ),
        organization_id: "test-org".to_string(),
        scaleway_base_url: "http://localhost:0".to_string(),
        metrics,
        last_reconcile_at: AtomicI64::new(last_reconcile_at),
    })
}

fn build_app(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics_handler))
        .route("/log-level", get(log_level))
        .with_state(state)
}

async fn body_string(body: Body) -> String {
    let bytes = body.collect().await.expect("failed to collect body").to_bytes();
    String::from_utf8_lossy(&bytes).into_owned()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// GET /healthz always returns 200 OK with body "ok".
#[tokio::test]
async fn test_healthz_returns_200() {
    let (registry, metrics) = build_registry_and_metrics();
    let ctx = build_context_with_metrics(metrics, 0);
    let app = build_app(AppState { ctx, registry });

    let response = app
        .oneshot(Request::builder().uri("/healthz").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_string(response.into_body()).await;
    assert_eq!(body, "ok");
}

/// GET /readyz with last_reconcile_at=0 returns 503 (never reconciled).
#[tokio::test]
async fn test_readyz_never_reconciled_returns_503() {
    let (registry, metrics) = build_registry_and_metrics();
    let ctx = build_context_with_metrics(metrics, 0);
    let app = build_app(AppState { ctx, registry });

    let response = app
        .oneshot(Request::builder().uri("/readyz").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

/// GET /readyz with last_reconcile_at set to now returns 200.
#[tokio::test]
async fn test_readyz_recent_reconcile_returns_200() {
    let (registry, metrics) = build_registry_and_metrics();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let ctx = build_context_with_metrics(metrics, now);
    let app = build_app(AppState { ctx, registry });

    let response = app
        .oneshot(Request::builder().uri("/readyz").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

/// GET /readyz with last_reconcile_at set to 61s ago returns 503.
#[tokio::test]
async fn test_readyz_stale_reconcile_returns_503() {
    let (registry, metrics) = build_registry_and_metrics();
    let stale = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        - 61;
    let ctx = build_context_with_metrics(metrics, stale);
    let app = build_app(AppState { ctx, registry });

    let response = app
        .oneshot(Request::builder().uri("/readyz").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

/// GET /metrics returns 200 and body contains the error counter metric name.
#[tokio::test]
async fn test_metrics_returns_200_with_metric_name() {
    let (registry, metrics) = build_registry_and_metrics();
    // Touch one counter label so prometheus has something to encode.
    metrics.record_error(&scaleway_operator::error::OperatorError::ConfigError(
        "test".to_string(),
    ));
    let ctx = build_context_with_metrics(metrics, 0);
    let app = build_app(AppState { ctx, registry });

    let response = app
        .oneshot(Request::builder().uri("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_string(response.into_body()).await;
    assert!(
        body.contains("scaleway_operator_reconcile_errors_total"),
        "expected metric name in body, got:\n{}",
        body
    );
}

/// GET /metrics Content-Type header is text/plain; version=0.0.4.
#[tokio::test]
async fn test_metrics_content_type() {
    let (registry, metrics) = build_registry_and_metrics();
    // Record a metric so the encoder produces non-empty output.
    metrics.record_error(&scaleway_operator::error::OperatorError::Unknown(
        "ct-test".to_string(),
    ));
    let ctx = build_context_with_metrics(metrics, 0);
    let app = build_app(AppState { ctx, registry });

    let response = app
        .oneshot(Request::builder().uri("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();

    let content_type = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(content_type, "text/plain; version=0.0.4");
}

/// GET /log-level returns 200 with a non-empty body.
#[tokio::test]
async fn test_log_level_returns_200_non_empty() {
    let (registry, metrics) = build_registry_and_metrics();
    let ctx = build_context_with_metrics(metrics, 0);
    let app = build_app(AppState { ctx, registry });

    let response = app
        .oneshot(Request::builder().uri("/log-level").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_string(response.into_body()).await;
    assert!(!body.is_empty(), "log-level body must not be empty");
}

/// GET /unknown returns 404.
#[tokio::test]
async fn test_unknown_path_returns_404() {
    let (registry, metrics) = build_registry_and_metrics();
    let ctx = build_context_with_metrics(metrics, 0);
    let app = build_app(AppState { ctx, registry });

    let response = app
        .oneshot(
            Request::builder()
                .uri("/this-path-does-not-exist")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
