/// Tests d'intégration pour reconcile_instance.
///
/// Exécution : `make test-integration-kind` (crée un cluster kind éphémère)
///
/// Les namespaces, NamespaceRoles et Secrets sont pré-créés par `k8s/test-fixtures.yaml`.
/// Les tests ne créent que des objets Instance (et les suppriment en fin de test).
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{DeleteParams, Patch, PatchParams, PostParams};
use kube::{Api, Client};
use scaleway_operator::{
    context::Context,
    resources::{
        Instance, InstanceSpec, InstanceStatus, KubernetesSecretRef, LoadBalancer,
        LoadBalancerSpec, ScalewaySecret, ScalewaySecretSpec, SecretSource,
    },
    scaleway::ScalewayClient,
};
use std::sync::Arc;

// ── Namespaces pré-créés par k8s/test-fixtures.yaml ──────────────────────────
/// Namespace avec annotation UUID valide, sans NamespaceRole.
const NS_NO_ROLE: &str = "scw-test-no-role";
/// Namespace sans annotation scaleway.mathieubodin.io/project-id, avec NamespaceRole Editor.
const NS_NO_ANNOTATION: &str = "scw-test-no-annotation";
/// Namespace avec annotation non-UUID, avec NamespaceRole Editor.
const NS_INVALID_UUID: &str = "scw-test-invalid-uuid";
/// Namespace Editor avec annotation valide, sans Secret IAM.
const NS_NO_SECRET: &str = "scw-test-no-secret";
/// Namespace Viewer avec annotation valide et Secret IAM.
const NS_VIEWER: &str = "scw-test-viewer";
/// Namespace Editor avec annotation valide et Secret IAM.
const NS_EDITOR: &str = "scw-test-editor";

const INSTANCE_FINALIZER: &str = "scaleway.mathieubodin.io/instance-finalizer";
const SECRET_FINALIZER: &str = "scaleway.mathieubodin.io/secret-finalizer";

// ── Noms des Secrets K8s pré-créés par k8s/test-fixtures.yaml (issue #118) ──
/// Secret K8s avec label opt-in + annotation OK + clé "password" présente.
/// Réservé au test happy path (`test_scalewaysecret_create_with_mock_scaleway_writes_status`,
/// `unimplemented!()` — voir #118).
#[allow(dead_code)]
const KS_SECRET_OPTED_IN: &str = "scw-test-secret-opted-in";
/// Secret K8s sans label opt-in → SecretOptInMissing permanent.
const KS_SECRET_NO_OPTIN: &str = "scw-test-secret-no-optin";
/// Secret K8s opt-in OK mais clé "password" absente → SecretKeyNotFound permanent.
const KS_SECRET_WRONG_KEY: &str = "scw-test-secret-wrong-key";

// ── Helpers ───────────────────────────────────────────────────────────────────

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

/// Génère un nom d'Instance unique pour éviter les collisions entre tests parallèles.
fn unique_name(prefix: &str) -> String {
    format!("{}-{}", prefix, &uuid::Uuid::new_v4().to_string()[..8])
}

/// Construit un objet Instance en mémoire (sans finalizer, sans status).
fn build_instance(ns: &str, name: &str) -> Instance {
    Instance {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            ..Default::default()
        },
        spec: InstanceSpec {
            name: name.to_string(),
            zone: "fr-par-1".to_string(),
            image: "ubuntu-jammy".to_string(),
            instance_type: "DEV1-S".to_string(),
            tags: vec![],
            boot_volume_size: 20,
            network: None,
            security: None,
        },
        status: None,
    }
}

/// Construit un objet Instance en mémoire avec le finalizer pré-présent.
fn build_instance_with_finalizer(ns: &str, name: &str) -> Instance {
    let mut instance = build_instance(ns, name);
    instance.metadata.finalizers = Some(vec![INSTANCE_FINALIZER.to_string()]);
    instance
}

// ── TestFixture ───────────────────────────────────────────────────────────────

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

    /// Crée une Instance dans k8s et la retourne (re-fetched pour avoir resourceVersion).
    pub async fn create_instance(&self, name: &str) -> Instance {
        let api: Api<Instance> = Api::namespaced(self.client.clone(), self.ns);
        let obj = build_instance_with_finalizer(self.ns, name);
        api.create(&PostParams::default(), &obj)
            .await
            .unwrap_or_else(|e| panic!("create_instance({}) failed: {}", name, e));
        api.get(name)
            .await
            .unwrap_or_else(|e| panic!("get after create({}) failed: {}", name, e))
    }

    /// Crée une Instance dans k8s, patche son status, retourne l'objet à jour.
    pub async fn create_instance_with_status(
        &self,
        name: &str,
        status: InstanceStatus,
    ) -> Instance {
        let api: Api<Instance> = Api::namespaced(self.client.clone(), self.ns);
        let obj = build_instance_with_finalizer(self.ns, name);
        api.create(&PostParams::default(), &obj)
            .await
            .unwrap_or_else(|e| panic!("create_instance_with_status({}) failed: {}", name, e));

        let patch = serde_json::json!({ "status": status });
        api.patch_status(name, &PatchParams::default(), &Patch::Merge(patch))
            .await
            .unwrap_or_else(|e| panic!("patch_status({}) failed: {}", name, e));

        api.get(name)
            .await
            .unwrap_or_else(|e| panic!("get after patch_status({}) failed: {}", name, e))
    }

    /// Crée un ScalewaySecret référençant un Secret K8s du namespace fixture (issue #118).
    ///
    /// `name`            : nom du CR ScalewaySecret (et du Secret Scaleway côté API).
    /// `k8s_secret_name` : nom du Secret K8s source dans le même namespace.
    /// `key`             : clé du Secret K8s à synchroniser.
    pub async fn create_scaleway_secret(
        &self,
        name: &str,
        k8s_secret_name: &str,
        key: &str,
    ) -> ScalewaySecret {
        let api: Api<ScalewaySecret> = Api::namespaced(self.client.clone(), self.ns);
        let obj = ScalewaySecret {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(self.ns.to_string()),
                finalizers: Some(vec![SECRET_FINALIZER.to_string()]),
                ..Default::default()
            },
            spec: ScalewaySecretSpec {
                name: name.to_string(),
                region: "fr-par".to_string(),
                source: SecretSource {
                    kubernetes_secret: Some(KubernetesSecretRef {
                        name: k8s_secret_name.to_string(),
                        key: key.to_string(),
                    }),
                },
                description: None,
                tags: vec![],
            },
            status: None,
        };
        api.create(&PostParams::default(), &obj)
            .await
            .unwrap_or_else(|e| panic!("create_scaleway_secret({}) failed: {}", name, e));
        api.get(name)
            .await
            .unwrap_or_else(|e| panic!("get after create({}) failed: {}", name, e))
    }

    /// Supprime un ScalewaySecret (retire d'abord le finalizer pour ne pas bloquer la GC).
    pub async fn cleanup_scaleway_secret(&self, name: &str) {
        let api: Api<ScalewaySecret> = Api::namespaced(self.client.clone(), self.ns);
        let remove_finalizer = serde_json::json!({ "metadata": { "finalizers": null } });
        let _ = api
            .patch(
                name,
                &PatchParams::default(),
                &Patch::Merge(remove_finalizer),
            )
            .await;
        let _ = api.delete(name, &DeleteParams::default()).await;
    }

    /// Supprime une Instance (retire d'abord le finalizer pour ne pas bloquer la GC).
    pub async fn cleanup_instance(&self, name: &str) {
        let api: Api<Instance> = Api::namespaced(self.client.clone(), self.ns);
        let remove_finalizer = serde_json::json!({ "metadata": { "finalizers": null } });
        let _ = api
            .patch(
                name,
                &PatchParams::default(),
                &Patch::Merge(remove_finalizer),
            )
            .await;
        let _ = api.delete(name, &DeleteParams::default()).await;
    }
}

// ── U4 : prérequis Kubernetes manquants ──────────────────────────────────────

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn test_missing_namespace_role_returns_config_error() {
    let server = mockito::Server::new_async().await;
    let fixture = TestFixture::for_namespace(NS_NO_ROLE).await;
    let ctx = fixture.ctx(&server.url());

    let result = scaleway_operator::reconcilers::reconcile_instance(
        Arc::new(build_instance(NS_NO_ROLE, "any")),
        ctx,
    )
    .await;

    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("No NamespaceRole found"),
        "Got: {}",
        err
    );
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn test_missing_project_id_annotation_returns_config_error() {
    let server = mockito::Server::new_async().await;
    let fixture = TestFixture::for_namespace(NS_NO_ANNOTATION).await;
    let ctx = fixture.ctx(&server.url());

    let result = scaleway_operator::reconcilers::reconcile_instance(
        Arc::new(build_instance(NS_NO_ANNOTATION, "any")),
        ctx,
    )
    .await;

    let err = result.unwrap_err();
    assert!(
        err.to_string()
            .contains("scaleway.mathieubodin.io/project-id"),
        "Got: {}",
        err
    );
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn test_invalid_uuid_annotation_returns_config_error() {
    let server = mockito::Server::new_async().await;
    let fixture = TestFixture::for_namespace(NS_INVALID_UUID).await;
    let ctx = fixture.ctx(&server.url());

    let result = scaleway_operator::reconcilers::reconcile_instance(
        Arc::new(build_instance(NS_INVALID_UUID, "any")),
        ctx,
    )
    .await;

    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("must be a valid UUID"),
        "Got: {}",
        err
    );
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn test_missing_iam_secret_returns_config_error() {
    // Finalizer pré-présent obligatoire : l'étape 4 requeue sinon et n'atteint jamais l'étape 6
    let server = mockito::Server::new_async().await;
    let fixture = TestFixture::for_namespace(NS_NO_SECRET).await;
    let ctx = fixture.ctx(&server.url());

    let result = scaleway_operator::reconcilers::reconcile_instance(
        Arc::new(build_instance_with_finalizer(NS_NO_SECRET, "any")),
        ctx,
    )
    .await;

    let err = result.unwrap_err();
    assert!(
        err.to_string()
            .contains("not found in namespace 'scaleway-system'"),
        "Got: {}",
        err
    );
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn test_viewer_role_cannot_create_instance() {
    // Finalizer pré-présent obligatoire pour atteindre l'étape 8 (vérification read-only)
    // Pas de mock Scaleway : le réconciliateur retourne Err avant verify_project_access
    let server = mockito::Server::new_async().await;
    let fixture = TestFixture::for_namespace(NS_VIEWER).await;
    let ctx = fixture.ctx(&server.url());

    let result = scaleway_operator::reconcilers::reconcile_instance(
        Arc::new(build_instance_with_finalizer(NS_VIEWER, "any")),
        ctx,
    )
    .await;

    let err = result.unwrap_err();
    assert!(err.to_string().contains("read-only"), "Got: {}", err);
}

// ── U5 : lifecycle du finalizer ───────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn test_finalizer_added_on_first_reconcile() {
    // Aucun mock Scaleway — le réconciliateur retourne à l'étape 4 avant les appels réseau
    let server = mockito::Server::new_async().await;
    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("finalizer");
    let ctx = fixture.ctx(&server.url());

    // Instance sans finalizer dans k8s
    let api: Api<Instance> = Api::namespaced(fixture.client.clone(), NS_EDITOR);
    let obj = build_instance(NS_EDITOR, &name);
    api.create(&PostParams::default(), &obj)
        .await
        .expect("Failed to create Instance");
    let fetched = api.get(&name).await.expect("Failed to fetch Instance");

    let result = scaleway_operator::reconcilers::reconcile_instance(Arc::new(fetched), ctx).await;

    let updated = api.get(&name).await.expect("Failed to re-fetch Instance");
    fixture.cleanup_instance(&name).await;
    drop(server);

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    let finalizers = updated.metadata.finalizers.unwrap_or_default();
    assert!(
        finalizers.contains(&INSTANCE_FINALIZER.to_string()),
        "Expected finalizer, got: {:?}",
        finalizers
    );
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn test_deletion_with_scaleway_id_calls_delete_api() {
    // handle_deletion utilise ctx.scaleway_client — mock sur le même server que fixture.ctx()
    let mut server = mockito::Server::new_async().await;
    let mock_delete = server
        .mock("DELETE", "/instance/v1/zones/fr-par-1/servers/srv-del-123")
        .with_status(204)
        .with_body("")
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("del-with-id");
    let status = InstanceStatus {
        scaleway_id: Some("srv-del-123".to_string()),
        ..Default::default()
    };
    let mut fetched = fixture.create_instance_with_status(&name, status).await;

    // Simuler la suppression via deletion_timestamp
    fetched.metadata.deletion_timestamp = Some(
        k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(k8s_openapi::jiff::Timestamp::now()),
    );

    let ctx = fixture.ctx(&server.url());
    let result = scaleway_operator::reconcilers::reconcile_instance(Arc::new(fetched), ctx).await;

    let api: Api<Instance> = Api::namespaced(fixture.client.clone(), NS_EDITOR);
    let updated = api.get(&name).await.expect("Failed to re-fetch Instance");
    fixture.cleanup_instance(&name).await;
    mock_delete.assert_async().await;
    drop(server);

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    let finalizers = updated.metadata.finalizers.clone().unwrap_or_default();
    assert!(
        !finalizers.contains(&INSTANCE_FINALIZER.to_string()),
        "Expected finalizer removed, got: {:?}",
        finalizers
    );
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn test_deletion_without_scaleway_id_removes_finalizer_only() {
    // Pas d'appel Scaleway — scaleway_id absent
    let server = mockito::Server::new_async().await;
    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("del-no-id");
    let mut fetched = fixture.create_instance(&name).await;

    fetched.metadata.deletion_timestamp = Some(
        k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(k8s_openapi::jiff::Timestamp::now()),
    );

    let ctx = fixture.ctx(&server.url());
    let result = scaleway_operator::reconcilers::reconcile_instance(Arc::new(fetched), ctx).await;

    let api: Api<Instance> = Api::namespaced(fixture.client.clone(), NS_EDITOR);
    let updated = api.get(&name).await.expect("Failed to re-fetch Instance");
    fixture.cleanup_instance(&name).await;
    drop(server);

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    let finalizers = updated.metadata.finalizers.clone().unwrap_or_default();
    assert!(
        !finalizers.contains(&INSTANCE_FINALIZER.to_string()),
        "Expected finalizer removed, got: {:?}",
        finalizers
    );
}

// ── U6 : création et synchronisation d'instance ───────────────────────────────

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn test_create_instance_writes_scaleway_id_to_status() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock(
            "GET",
            "/account/v3/projects/11111111-1111-1111-1111-111111111111",
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"id": "11111111-1111-1111-1111-111111111111"}"#)
        .create_async()
        .await;
    server
        .mock(
            "GET",
            mockito::Matcher::Regex(r"/instance/v1/zones/fr-par-1/servers".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"servers": []}"#)
        .create_async()
        .await;
    let mock_create = server
        .mock("POST", "/instance/v1/zones/fr-par-1/servers")
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(r#"{"server": {"id": "srv-new-123"}}"#)
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("create");
    let fetched = fixture.create_instance(&name).await;
    let ctx = fixture.ctx(&server.url());

    let result = scaleway_operator::reconcilers::reconcile_instance(Arc::new(fetched), ctx).await;

    let api: Api<Instance> = Api::namespaced(fixture.client.clone(), NS_EDITOR);
    let updated = api.get(&name).await.expect("Failed to re-fetch Instance");
    fixture.cleanup_instance(&name).await;
    mock_create.assert_async().await;
    drop(server);

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    let status = updated.status.expect("Expected status");
    assert_eq!(status.scaleway_id, Some("srv-new-123".to_string()));
    assert_eq!(status.sync_state, "Syncing");
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn test_orphan_adoption_does_not_call_create() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock(
            "GET",
            "/account/v3/projects/11111111-1111-1111-1111-111111111111",
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"id": "11111111-1111-1111-1111-111111111111"}"#)
        .create_async()
        .await;
    // find_instance_by_name retourne une instance orpheline
    server
        .mock(
            "GET",
            mockito::Matcher::Regex(r"/instance/v1/zones/fr-par-1/servers".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"servers": [{"id": "srv-orphan-456"}]}"#)
        .create_async()
        .await;
    // Pas de mock POST — create ne doit PAS être appelé

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("orphan");
    let fetched = fixture.create_instance(&name).await;
    let ctx = fixture.ctx(&server.url());

    let result = scaleway_operator::reconcilers::reconcile_instance(Arc::new(fetched), ctx).await;

    let api: Api<Instance> = Api::namespaced(fixture.client.clone(), NS_EDITOR);
    let updated = api.get(&name).await.expect("Failed to re-fetch Instance");
    fixture.cleanup_instance(&name).await;
    drop(server);

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    let status = updated.status.expect("Expected status");
    assert_eq!(status.scaleway_id, Some("srv-orphan-456".to_string()));
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn test_sync_updates_state_and_public_ip() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock(
            "GET",
            mockito::Matcher::Regex(
                r"/instance/v1/zones/fr-par-1/servers/srv-running-789".to_string(),
            ),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"server": {"id": "srv-running-789", "state": "running", "public_ip": {"address": "1.2.3.4"}, "creation_date": "2026-01-01T00:00:00Z"}}"#)
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("sync");
    let status = InstanceStatus {
        scaleway_id: Some("srv-running-789".to_string()),
        project_id: Some("11111111-1111-1111-1111-111111111111".to_string()),
        ..Default::default()
    };
    let fetched = fixture.create_instance_with_status(&name, status).await;
    let ctx = fixture.ctx(&server.url());

    let result = scaleway_operator::reconcilers::reconcile_instance(Arc::new(fetched), ctx).await;

    let api: Api<Instance> = Api::namespaced(fixture.client.clone(), NS_EDITOR);
    let updated = api.get(&name).await.expect("Failed to re-fetch Instance");
    fixture.cleanup_instance(&name).await;
    drop(server);

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    let status = updated.status.expect("Expected status");
    assert_eq!(status.state, "running");
    assert_eq!(status.public_ip, Some("1.2.3.4".to_string()));
    assert_eq!(status.sync_state, "Synced");
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn test_scaleway_error_sets_sync_state_error() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock(
            "GET",
            mockito::Matcher::Regex(
                r"/instance/v1/zones/fr-par-1/servers/srv-error-000".to_string(),
            ),
        )
        .with_status(500)
        .with_body(r#"{"message": "internal error"}"#)
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("error");
    let status = InstanceStatus {
        scaleway_id: Some("srv-error-000".to_string()),
        project_id: Some("11111111-1111-1111-1111-111111111111".to_string()),
        ..Default::default()
    };
    let fetched = fixture.create_instance_with_status(&name, status).await;
    let ctx = fixture.ctx(&server.url());

    let result = scaleway_operator::reconcilers::reconcile_instance(Arc::new(fetched), ctx).await;

    let api: Api<Instance> = Api::namespaced(fixture.client.clone(), NS_EDITOR);
    let updated = api.get(&name).await.expect("Failed to re-fetch Instance");
    fixture.cleanup_instance(&name).await;
    drop(server);

    assert!(result.is_err(), "Expected Err on Scaleway 500");
    let status = updated.status.expect("Expected status");
    assert_eq!(status.sync_state, "Error");
}

// ── LoadBalancer integration tests ────────────────────────────────────────────
//
// Prérequis : même que les tests Instance.
// Ces tests vérifient la réconciliation de bout en bout du LoadBalancer.

fn build_load_balancer(ns: &str, name: &str) -> LoadBalancer {
    LoadBalancer {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            ..Default::default()
        },
        spec: LoadBalancerSpec {
            name: name.to_string(),
            zone: "fr-par-1".to_string(),
            lb_type: "LB-S".to_string(),
            description: None,
            tags: vec![],
        },
        status: None,
    }
}

#[tokio::test]
#[ignore = "requires make test-integration-kind"]
async fn test_loadbalancer_adds_finalizer_on_first_reconcile() {
    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let server = mockito::Server::new_async().await;

    // Mock NamespaceRole lookup (handled by kube API, not mocked here)
    // The reconciler should add the finalizer on the first reconcile cycle.

    let name = unique_name("lb-test");
    let lb_api: Api<LoadBalancer> = Api::namespaced(fixture.client.clone(), NS_EDITOR);

    let lb = lb_api
        .create(
            &PostParams::default(),
            &build_load_balancer(NS_EDITOR, &name),
        )
        .await
        .expect("Failed to create LoadBalancer CR");

    let ctx = fixture.ctx(&server.url());
    let result = scaleway_operator::reconcilers::reconcile_load_balancer(Arc::new(lb), ctx).await;

    let updated = lb_api
        .get(&name)
        .await
        .expect("Failed to re-fetch LoadBalancer");
    let _ = lb_api.delete(&name, &DeleteParams::default()).await;
    drop(server);

    // AddFinalizer returns Ok(requeue 5s)
    assert!(
        result.is_ok(),
        "Expected Ok on first reconcile, got {:?}",
        result
    );
    assert!(
        updated
            .metadata
            .finalizers
            .unwrap_or_default()
            .contains(&"scaleway.mathieubodin.io/loadbalancer-finalizer".to_string()),
        "Finalizer should be present after first reconcile"
    );
}

// ── ScalewaySecret integration tests (issue #118 — scaffolding) ───────────────
//
// Ces tests sont des squelettes pour démontrer la forme attendue ; ils sont
// `#[ignore]` car ils nécessitent un cluster avec les fixtures de la section
// "Fixtures ScalewaySecret" de k8s/test-fixtures.yaml ET un serveur mock
// Scaleway pour le cas de création. La logique métier (opt-in, clé manquante,
// création + revision) est déjà couverte par les tests unitaires de
// reconcilers.rs ; ces tests fermeraient les bornes du contrat
// `last_synced_resource_version == ks.metadata.resource_version` (SEC-002)
// et de la cascade Create + create_secret_version.
//
// Implémentation complète à faire dans une PR de suivi — l'effort serait
// déraisonnable pour cette PR (setup mockito multi-endpoints + cycle status
// Scaleway). Le scaffolding ici sert de point d'entrée concret.

#[tokio::test]
#[ignore = "see #118 — needs cluster fixtures + mock Scaleway secret API"]
async fn test_scalewaysecret_opt_in_missing_returns_permanent_error() {
    // Cas : CR ScalewaySecret pointe vers un Secret K8s sans label opt-in.
    // Attendu : Err(SecretOptInMissing) — permanent (await_change, pas de retry).
    let server = mockito::Server::new_async().await;
    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-no-optin");
    let _cr = fixture
        .create_scaleway_secret(&name, KS_SECRET_NO_OPTIN, "password")
        .await;
    let ctx = fixture.ctx(&server.url());

    let api: Api<ScalewaySecret> = Api::namespaced(fixture.client.clone(), NS_EDITOR);
    let fetched = api.get(&name).await.expect("re-fetch ScalewaySecret");
    let result =
        scaleway_operator::reconcilers::reconcile_scaleway_secret(Arc::new(fetched), ctx).await;

    fixture.cleanup_scaleway_secret(&name).await;
    drop(server);

    let err = result.expect_err("expected SecretOptInMissing");
    assert!(
        err.to_string().contains("opt-in"),
        "expected opt-in error, got: {}",
        err
    );
}

#[tokio::test]
#[ignore = "see #118 — needs cluster fixtures + mock Scaleway secret API"]
async fn test_scalewaysecret_key_missing_returns_permanent_error() {
    // Cas : CR ScalewaySecret pointe vers un Secret K8s opt-in OK mais clé
    // référencée absente de .data. Attendu : Err(SecretKeyNotFound) — permanent.
    let server = mockito::Server::new_async().await;
    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = "scw-secret-cr-2".to_string(); // annotation allowed-cr cible ce nom
    let _cr = fixture
        .create_scaleway_secret(&name, KS_SECRET_WRONG_KEY, "password")
        .await;
    let ctx = fixture.ctx(&server.url());

    let api: Api<ScalewaySecret> = Api::namespaced(fixture.client.clone(), NS_EDITOR);
    let fetched = api.get(&name).await.expect("re-fetch ScalewaySecret");
    let result =
        scaleway_operator::reconcilers::reconcile_scaleway_secret(Arc::new(fetched), ctx).await;

    fixture.cleanup_scaleway_secret(&name).await;
    drop(server);

    let err = result.expect_err("expected SecretKeyNotFound");
    assert!(
        err.to_string().to_lowercase().contains("key"),
        "expected key-not-found error, got: {}",
        err
    );
}

#[tokio::test]
#[ignore = "see #118 — needs cluster fixtures + mock Scaleway secret API (POST /secrets + POST /versions)"]
async fn test_scalewaysecret_create_with_mock_scaleway_writes_status() {
    // Happy path : K8s Secret opt-in OK + clé présente, Scaleway mock répond
    // 201 sur POST /secrets et POST .../versions.
    // Attendu : status.scaleway_id = "sec-mock-id"
    //           status.last_synced_resource_version = rv du Secret K8s
    //           sync_state = "Synced"
    //
    // Verrouille le contrat SEC-002 : `last_synced_resource_version` provient
    // de `ks.metadata.resource_version` (et non pas d'un hash de la valeur).
    let mut server = mockito::Server::new_async().await;
    server
        .mock(
            "GET",
            "/account/v3/projects/11111111-1111-1111-1111-111111111111",
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"id": "11111111-1111-1111-1111-111111111111"}"#)
        .create_async()
        .await;
    // TODO(#118) : compléter les mocks find_by_tags + create_secret + create_secret_version
    // et asserter status.last_synced_resource_version == rv du Secret K8s lu.
    unimplemented!("see #118 — needs full Scaleway secret API mock surface");
}
