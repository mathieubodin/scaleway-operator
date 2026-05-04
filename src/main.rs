use futures::StreamExt;
use kube::runtime::Controller;
use kube::{Api, Client};
use scaleway_operator::{
    context::Context,
    reconcilers::{error_policy, reconcile_instance},
    resources::Instance,
    scaleway::ScalewayClient,
};
use std::sync::Arc;

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

    let context = Arc::new(Context {
        client: client.clone(),
        scaleway_client: ScalewayClient::new(scaleway_token),
        organization_id,
    });

    tracing::debug!(org_id = %context.organization_id, "Initialized Scaleway operator");

    tokio::spawn(run_health_server());

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

async fn run_health_server() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("0.0.0.0:8080")
        .await
        .expect("failed to bind health server to :8080");

    tracing::info!("Health server listening on :8080");

    loop {
        let Ok((mut stream, _)) = listener.accept().await else {
            continue;
        };
        tokio::spawn(async move {
            let mut buf = [0u8; 256];
            let _ = stream.read(&mut buf).await;
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                .await;
        });
    }
}
