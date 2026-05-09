use axum::{extract::State, routing::get, Router};
use axum::http::StatusCode;
use futures::StreamExt;
use kube::runtime::Controller;
use kube::{Api, Client};
use prometheus::{Encoder, TextEncoder};
use scaleway_operator::{
    context::Context,
    reconcilers::{error_policy, reconcile_instance},
    resources::Instance,
    scaleway::ScalewayClient,
};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
struct AppState {
    ctx: Arc<Context>,
    registry: Arc<prometheus::Registry>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("scaleway_operator=info".parse()?),
        )
        .init();

    tracing::info!("Starting Scaleway Operator");

    let client = Client::try_default().await?;

    let scaleway_token =
        std::env::var("SCALEWAY_TOKEN").expect("SCALEWAY_TOKEN env var must be set");
    let organization_id =
        std::env::var("SCALEWAY_ORG_ID").expect("SCALEWAY_ORG_ID env var must be set");

    let registry = prometheus::Registry::new();
    let metrics = scaleway_operator::metrics::OperatorMetrics::new(&registry)
        .expect("failed to register metrics");
    let registry = Arc::new(registry);

    let context = Arc::new(Context {
        client: client.clone(),
        scaleway_client: ScalewayClient::new(scaleway_token),
        organization_id,
        scaleway_base_url: "https://api.scaleway.com".to_string(),
        metrics,
        last_reconcile_at: std::sync::atomic::AtomicI64::new(0),
    });

    tracing::debug!(org_id = %context.organization_id, "Initialized Scaleway operator");

    tokio::spawn(run_axum_server(Arc::clone(&context), Arc::clone(&registry)));

    let api = Api::<Instance>::all(client);
    Controller::new(api, Default::default())
        .run(reconcile_instance, error_policy, context)
        .for_each(|res| async move {
            if let Err(e) = res {
                tracing::error!(error = %e, "Reconciliation failed");
            }
        })
        .await;

    Ok(())
}

async fn run_axum_server(ctx: Arc<Context>, registry: Arc<prometheus::Registry>) {
    let state = AppState { ctx, registry };

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics_handler))
        .route("/log-level", get(log_level))
        .with_state(state);

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

async fn metrics_handler(State(state): State<AppState>) -> (StatusCode, [(axum::http::HeaderName, &'static str); 1], String) {
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
