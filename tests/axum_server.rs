/// Unit tests for the axum HTTP server handlers (U4).
///
/// Uses `tower::ServiceExt::oneshot` to call handlers without a live TCP listener.
/// Imports the real handlers from `scaleway_operator::server` — no duplication.
use axum::{body::Body, http::{Request, StatusCode}};
use http_body_util::BodyExt;
use scaleway_operator::{
    context::Context,
    metrics::OperatorMetrics,
    scaleway::ScalewayClient,
    server::{AppState, build_router},
};
use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::time::{SystemTime, UNIX_EPOCH};
use tower::ServiceExt;

fn build_registry_and_metrics() -> (Arc<prometheus::Registry>, OperatorMetrics) {
    let registry = prometheus::Registry::new();
    let metrics = OperatorMetrics::new(&registry).expect("metrics registration failed");
    (Arc::new(registry), metrics)
}

fn build_context_with_metrics(metrics: OperatorMetrics, last_reconcile_at: i64) -> Arc<Context> {
    Arc::new(Context {
        client: {
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

fn unix_now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

async fn body_string(body: Body) -> String {
    let bytes = body.collect().await.expect("failed to collect body").to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap_or_default()
}

#[tokio::test]
async fn test_healthz_returns_200() {
    let (registry, metrics) = build_registry_and_metrics();
    let ctx = build_context_with_metrics(metrics, 0);
    let app = build_router(AppState { ctx, registry });

    let response = app
        .oneshot(Request::builder().uri("/healthz").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_string(response.into_body()).await, "ok");
}

#[tokio::test]
async fn test_readyz_never_reconciled_returns_503() {
    let (registry, metrics) = build_registry_and_metrics();
    let ctx = build_context_with_metrics(metrics, 0);
    let app = build_router(AppState { ctx, registry });

    let response = app
        .oneshot(Request::builder().uri("/readyz").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn test_readyz_recent_reconcile_returns_200() {
    let (registry, metrics) = build_registry_and_metrics();
    let ctx = build_context_with_metrics(metrics, unix_now_secs());
    let app = build_router(AppState { ctx, registry });

    let response = app
        .oneshot(Request::builder().uri("/readyz").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_readyz_stale_reconcile_returns_503() {
    let (registry, metrics) = build_registry_and_metrics();
    let ctx = build_context_with_metrics(metrics, unix_now_secs() - 61);
    let app = build_router(AppState { ctx, registry });

    let response = app
        .oneshot(Request::builder().uri("/readyz").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn test_metrics_returns_200_with_metric_name() {
    let (registry, metrics) = build_registry_and_metrics();
    metrics.record_error(&scaleway_operator::error::OperatorError::ConfigError("test".to_string()));
    let ctx = build_context_with_metrics(metrics, 0);
    let app = build_router(AppState { ctx, registry });

    let response = app
        .oneshot(Request::builder().uri("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_string(response.into_body()).await;
    assert!(
        body.contains("scaleway_operator_reconcile_errors_total"),
        "expected metric name in body, got:\n{}", body
    );
}

#[tokio::test]
async fn test_metrics_content_type() {
    let (registry, metrics) = build_registry_and_metrics();
    metrics.record_error(&scaleway_operator::error::OperatorError::Unknown("ct-test".to_string()));
    let ctx = build_context_with_metrics(metrics, 0);
    let app = build_router(AppState { ctx, registry });

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

#[tokio::test]
async fn test_log_level_returns_200_non_empty() {
    let (registry, metrics) = build_registry_and_metrics();
    let ctx = build_context_with_metrics(metrics, 0);
    let app = build_router(AppState { ctx, registry });

    let response = app
        .oneshot(Request::builder().uri("/log-level").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(!body_string(response.into_body()).await.is_empty());
}

#[tokio::test]
async fn test_unknown_path_returns_404() {
    let (registry, metrics) = build_registry_and_metrics();
    let ctx = build_context_with_metrics(metrics, 0);
    let app = build_router(AppState { ctx, registry });

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
