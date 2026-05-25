use futures::StreamExt;
use kube::runtime::Controller;
use kube::{Api, Client};
use scaleway_operator::{
    context::{CircuitBreakerState, Context},
    reconcilers::{error_policy, reconcile_instance, reconcile_load_balancer},
    resources::{Instance, LoadBalancer},
    scaleway::ScalewayClient,
    server::run_axum_server,
};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
        last_reconcile_at: std::sync::atomic::AtomicI64::new(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
        ),
        retry_counts: std::sync::Mutex::new(std::collections::HashMap::new()),
        circuit_breaker: std::sync::Mutex::new(CircuitBreakerState::Closed { failure_count: 0 }),
    });

    tracing::debug!(org_id = %context.organization_id, "Initialized Scaleway operator");

    tokio::spawn(run_axum_server(Arc::clone(&context), Arc::clone(&registry)));

    // Heartbeat : rafraîchit last_reconcile_at toutes les 30s pour maintenir /readyz
    // même quand il n'y a aucune Instance à réconcilier.
    tokio::spawn({
        let ctx = Arc::clone(&context);
        async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            loop {
                interval.tick().await;
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                ctx.last_reconcile_at.store(now, Ordering::Release);
            }
        }
    });

    let instance_api = Api::<Instance>::all(client.clone());
    let lb_api = Api::<LoadBalancer>::all(client);

    let instance_ctrl = Controller::new(instance_api, Default::default())
        .run(
            reconcile_instance,
            |obj, err, ctx| error_policy("instance", obj, err, ctx),
            Arc::clone(&context),
        )
        .for_each(|res| async move {
            if let Err(e) = res {
                tracing::error!(error = %e, "Instance reconciliation failed");
            }
        });

    let lb_ctrl = Controller::new(lb_api, Default::default())
        .run(
            reconcile_load_balancer,
            |obj, err, ctx| error_policy("loadbalancer", obj, err, ctx),
            Arc::clone(&context),
        )
        .for_each(|res| async move {
            if let Err(e) = res {
                tracing::error!(error = %e, "LoadBalancer reconciliation failed");
            }
        });

    tokio::join!(instance_ctrl, lb_ctrl);

    Ok(())
}
