//! Connexion au cluster de test et contexte de réconciliation.

use kube::Client;
use scaleway_operator::{context::Context, scaleway::ScalewayClient};
use std::sync::Arc;

// ── Namespaces pré-créés par k8s/test-fixtures.yaml ──────────────────────────
/// Namespace avec annotation UUID valide, sans NamespaceRole.
pub const NS_NO_ROLE: &str = "scw-test-no-role";
/// Namespace sans annotation scaleway.mathieubodin.io/project-id, avec NamespaceRole Editor.
pub const NS_NO_ANNOTATION: &str = "scw-test-no-annotation";
/// Namespace avec annotation non-UUID, avec NamespaceRole Editor.
pub const NS_INVALID_UUID: &str = "scw-test-invalid-uuid";
/// Namespace Editor avec annotation valide, sans Secret IAM.
pub const NS_NO_SECRET: &str = "scw-test-no-secret";
/// Namespace Viewer avec annotation valide et Secret IAM.
pub const NS_VIEWER: &str = "scw-test-viewer";
/// Namespace Editor avec annotation valide et Secret IAM.
pub const NS_EDITOR: &str = "scw-test-editor";

/// Crée un kube::Client en respectant KUBE_API_URL (utile avec kubectl proxy).
/// `KUBE_API_URL=http://127.0.0.1:8001` est injecté par `make test-integration`.
async fn make_client() -> Client {
    match std::env::var("KUBE_API_URL") {
        Ok(url) => {
            let config = kube::Config::new(
                url.parse()
                    .unwrap_or_else(|_| panic!("KUBE_API_URL invalide : {}", url)),
            );
            Client::try_from(config).expect("Impossible de créer le client depuis KUBE_API_URL")
        }
        Err(_) => Client::try_default().await.expect(
            "Impossible de se connecter à Kubernetes. \
             Définir KUBE_API_URL=http://127.0.0.1:8001 si kubectl proxy est en cours.",
        ),
    }
}

/// Génère un nom de ressource unique pour éviter les collisions entre tests parallèles.
pub fn unique_name(prefix: &str) -> String {
    format!("{}-{}", prefix, &uuid::Uuid::new_v4().to_string()[..8])
}

pub struct TestFixture {
    pub client: Client,
    pub ns: &'static str,
}

impl TestFixture {
    pub async fn for_namespace(ns: &'static str) -> Self {
        let client = make_client().await;
        TestFixture { client, ns }
    }

    /// Construit un Context avec les deux clients pointant vers mock_url.
    pub fn ctx(&self, mock_url: &str) -> Arc<Context> {
        Arc::new(Context {
            client: self.client.clone(),
            scaleway_client: ScalewayClient::new_with_base_url(
                "test-token".to_string(),
                mock_url.to_string(),
            ),
            organization_id: "test-org".to_string(),
            scaleway_base_url: mock_url.to_string(),
            metrics: scaleway_operator::metrics::OperatorMetrics::new(&prometheus::Registry::new())
                .unwrap(),
            last_reconcile_at: std::sync::atomic::AtomicI64::new(0),
            retry_counts: std::sync::Mutex::new(std::collections::HashMap::new()),
            circuit_breaker: std::sync::Mutex::new(
                scaleway_operator::context::CircuitBreakerState::Closed { failure_count: 0 },
            ),
        })
    }
}
