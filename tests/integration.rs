use k8s_openapi::api::core::v1::Secret;
/// Tests d'intégration des reconcilers.
///
/// Exécution : `make test-integration-kind` (crée un cluster kind éphémère)
///
/// Les namespaces, NamespaceRoles et Secrets IAM sont pré-créés par `k8s/test-fixtures.yaml`.
/// Les tests créent leurs propres ressources (Instance, ScalewaySecret et Secret source)
/// et les suppriment en fin de test.
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::ByteString;
use kube::api::{DeleteParams, Patch, PatchParams, PostParams};
use kube::{Api, Client};
use scaleway_operator::{
    context::Context,
    error::OperatorError,
    resources::{
        Instance, InstanceSpec, InstanceStatus, KubernetesSecretRef, LoadBalancer,
        LoadBalancerSpec, ScalewaySecret, ScalewaySecretSpec, ScalewaySecretStatus, SecretSource,
    },
    scaleway::ScalewayClient,
};
use std::collections::BTreeMap;
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

const OPT_IN_LABEL: &str = "scaleway.mathieubodin.io/allow-operator-read";
const ALLOWED_CR_ANNOTATION: &str = "scaleway.mathieubodin.io/allowed-cr";
/// Chemin de l'API Secret Manager pour la région des tests.
const SECRETS_PATH: &str = "/secret-manager/v1beta1/regions/fr-par/secrets";

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
        self.create_scaleway_secret_in_region(name, k8s_secret_name, key, "fr-par")
            .await
    }

    pub async fn create_scaleway_secret_in_region(
        &self,
        name: &str,
        k8s_secret_name: &str,
        key: &str,
        region: &str,
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
                region: region.to_string(),
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

    /// Crée le Secret K8s source d'un ScalewaySecret, avec la clé `key`.
    /// `opt_in` pose le label et l'annotation qui autorisent le CR `cr_name` à le lire.
    /// Retourne son nom et son resourceVersion.
    pub async fn create_source_secret(
        &self,
        cr_name: &str,
        key: &str,
        opt_in: bool,
    ) -> (String, String) {
        let api: Api<Secret> = Api::namespaced(self.client.clone(), self.ns);
        let name = format!("{}-src", cr_name);
        let (labels, annotations) = if opt_in {
            (
                Some(BTreeMap::from([(
                    OPT_IN_LABEL.to_string(),
                    "true".to_string(),
                )])),
                Some(BTreeMap::from([(
                    ALLOWED_CR_ANNOTATION.to_string(),
                    format!("{}/{}", self.ns, cr_name),
                )])),
            )
        } else {
            (None, None)
        };
        let obj = Secret {
            metadata: ObjectMeta {
                name: Some(name.clone()),
                namespace: Some(self.ns.to_string()),
                labels,
                annotations,
                ..Default::default()
            },
            data: Some(BTreeMap::from([(
                key.to_string(),
                ByteString(b"s3cr3t".to_vec()),
            )])),
            ..Default::default()
        };
        let created = api
            .create(&PostParams::default(), &obj)
            .await
            .unwrap_or_else(|e| panic!("create_source_secret({}) failed: {}", name, e));
        let rv = created
            .metadata
            .resource_version
            .expect("source secret has a resourceVersion");
        (name, rv)
    }

    pub async fn cleanup_source_secret(&self, name: &str) {
        let api: Api<Secret> = Api::namespaced(self.client.clone(), self.ns);
        let _ = api.delete(name, &DeleteParams::default()).await;
    }

    /// Écrit le status d'un ScalewaySecret, pour partir d'un état déjà synchronisé.
    pub async fn set_scaleway_secret_status(&self, name: &str, status: ScalewaySecretStatus) {
        let api: Api<ScalewaySecret> = Api::namespaced(self.client.clone(), self.ns);
        let patch = serde_json::json!({ "status": status });
        api.patch_status(name, &PatchParams::default(), &Patch::Merge(patch))
            .await
            .unwrap_or_else(|e| panic!("set_scaleway_secret_status({}) failed: {}", name, e));
    }

    pub async fn get_scaleway_secret(&self, name: &str) -> ScalewaySecret {
        let api: Api<ScalewaySecret> = Api::namespaced(self.client.clone(), self.ns);
        api.get(name)
            .await
            .unwrap_or_else(|e| panic!("get_scaleway_secret({}) failed: {}", name, e))
    }

    /// Relit le CR puis lance une réconciliation, comme le ferait le controller.
    pub async fn reconcile_scaleway_secret(
        &self,
        name: &str,
        ctx: &Arc<Context>,
    ) -> Result<kube::runtime::controller::Action, OperatorError> {
        let fetched = self.get_scaleway_secret(name).await;
        scaleway_operator::reconcilers::reconcile_scaleway_secret(Arc::new(fetched), ctx.clone())
            .await
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

/// Scaffold pour le cycle de vie LoadBalancer E2E avec credentials Scaleway live.
///
/// Ce test a été supprimé en PR #113 (revue qualité) car son corps contenait un
/// `todo!()` trompeur — le test était marqué `#[ignore]` mais paniquait
/// silencieusement si jamais quelqu'un le levait. L'issue #119 demande de
/// restaurer un squelette **compilable, reviewable, et qui ne ment pas sur la
/// couverture LB**.
///
/// Tant que les credentials sandbox Scaleway avec `LoadBalancerFullAccess` ne
/// sont pas câblés en CI, le test reste `#[ignore]` ; même levé via
/// `--ignored`, il sort tôt si la variable d'env `SCALEWAY_LB_LIVE_TEST` n'est
/// pas définie (skip explicite, pas de panic). Les phases d'implémentation
/// restent tracées via `TODO(#119)`.
#[tokio::test]
#[ignore = "requires live Scaleway credentials with LB permissions — see #119"]
async fn test_loadbalancer_create_sync_delete() {
    // Pré-requis (à câbler par CI maintainer ou ops, voir issue #119) :
    // - Variable d'env SCALEWAY_LB_LIVE_TEST=1 pour signaler le mode live
    // - Secret K8s `scaleway-ns-creds-scw-test-editor` (déjà dans test-fixtures.yaml)
    //   contient une clé IAM Scaleway avec LoadBalancerFullAccess sur un projet sandbox
    // - Annotation `scaleway.mathieubodin.io/project-id` du namespace `scw-test-editor`
    //   pointe vers ce projet sandbox
    //
    // Flow :
    // 1. Créer un LoadBalancer CR avec un nom unique (pour éviter collisions)
    // 2. Réconcilier (boucle wait jusqu'à 60s, requeue 5s puis 30s)
    //    → status.scaleway_id doit être populated
    //    → status.state doit converger vers "ready" ou "running" selon le payload Scaleway
    //    → status.public_ip doit être présent
    // 3. Supprimer le CR
    // 4. Re-réconcilier (boucle wait jusqu'à 60s)
    //    → la suppression Scaleway doit être appelée (DELETE /lb/v1/zones/.../lbs/...)
    //    → le finalizer doit être retiré
    //    → le CR doit disparaître de l'API K8s
    //
    // ⚠️ Coût : l'exécution réelle consomme un LB Scaleway pendant ~1 minute.
    //    À exécuter dans un projet sandbox uniquement.

    if std::env::var("SCALEWAY_LB_LIVE_TEST").is_err() {
        eprintln!("test_loadbalancer_create_sync_delete: SCALEWAY_LB_LIVE_TEST non défini, skip");
        return;
    }

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-lb-live");
    let lb_api: Api<LoadBalancer> = Api::namespaced(fixture.client.clone(), NS_EDITOR);

    // Cleanup en garantie (cas où un test précédent a laissé un orphelin)
    let _ = lb_api.delete(&name, &DeleteParams::default()).await;

    // -- TODO(#119) -- Phase 1 : Apply CR + assertion status.scaleway_id Some
    // -- TODO(#119) -- Phase 2 : Wait state convergence + public_ip
    // -- TODO(#119) -- Phase 3 : Delete CR + assertion finalizer removed
    //
    // Implementation requires the `wait_for_predicate(timeout, interval, f)` helper
    // (not yet extracted). When this test is enabled, factor that helper out of
    // existing Instance integration tests for reuse.

    unimplemented!("test scaffold — see #119 for implementation tracking");
}

// ── ScalewaySecret : couche I/O du reconciler (issue #118) ───────────────────
//
// Chaque test crée son propre Secret source et son propre CR, pour pouvoir
// tourner en parallèle. L'API Secret Manager est simulée par mockito.

fn mock_json(server: &mut mockito::Server, method: &str, path: &str, body: &str) -> mockito::Mock {
    server
        .mock(method, path)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
}

/// Recherche par tags : la requête porte `project_id` et `tags` en paramètres.
fn mock_find_by_tags(server: &mut mockito::Server, body: &str) -> mockito::Mock {
    server
        .mock("GET", SECRETS_PATH)
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
}

fn synced_status(scaleway_id: &str, revision: u32, resource_version: &str) -> ScalewaySecretStatus {
    ScalewaySecretStatus {
        scaleway_id: Some(scaleway_id.to_string()),
        current_version: Some(revision),
        last_synced_resource_version: Some(resource_version.to_string()),
        sync_state: "Synced".to_string(),
        error_message: None,
    }
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn test_scalewaysecret_opt_in_missing_returns_permanent_error() {
    let server = mockito::Server::new_async().await;
    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-no-optin");
    let (source, _) = fixture.create_source_secret(&name, "password", false).await;
    fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let updated = fixture.get_scaleway_secret(&name).await;

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    let err = result.expect_err("expected SecretOptInMissing");
    assert!(
        matches!(err, OperatorError::SecretOptInMissing(_)),
        "got: {err:?}"
    );
    assert!(err.is_permanent_error());
    let status = updated.status.expect("Expected status");
    assert_eq!(status.sync_state, "Error");
    assert_eq!(status.scaleway_id, None);
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn test_scalewaysecret_key_missing_returns_permanent_error() {
    let server = mockito::Server::new_async().await;
    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-wrong-key");
    let (source, _) = fixture.create_source_secret(&name, "other", true).await;
    fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let updated = fixture.get_scaleway_secret(&name).await;

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    let err = result.expect_err("expected SecretKeyNotFound");
    assert!(
        matches!(err, OperatorError::SecretKeyNotFound(_)),
        "got: {err:?}"
    );
    assert!(err.is_permanent_error());
    assert_eq!(updated.status.expect("Expected status").sync_state, "Error");
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn test_scalewaysecret_create_writes_status_from_source_resource_version() {
    let mut server = mockito::Server::new_async().await;
    let find = mock_find_by_tags(&mut server, r#"{"secrets": []}"#)
        .expect(1)
        .create_async()
        .await;
    let create = mock_json(
        &mut server,
        "POST",
        SECRETS_PATH,
        r#"{"id": "sec-created"}"#,
    )
    .expect(1)
    .create_async()
    .await;
    let version = mock_json(
        &mut server,
        "POST",
        &format!("{SECRETS_PATH}/sec-created/versions"),
        r#"{"revision": 1}"#,
    )
    .expect(1)
    .create_async()
    .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-create");
    let (source, source_rv) = fixture.create_source_secret(&name, "password", true).await;
    fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let updated = fixture.get_scaleway_secret(&name).await;

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    find.assert_async().await;
    create.assert_async().await;
    version.assert_async().await;
    let status = updated.status.expect("Expected status");
    assert_eq!(status.scaleway_id.as_deref(), Some("sec-created"));
    assert_eq!(status.current_version, Some(1));
    assert_eq!(status.last_synced_resource_version, Some(source_rv));
    assert_eq!(status.sync_state, "Synced");
    assert_eq!(status.error_message, None);
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn test_scalewaysecret_adoption_does_not_call_create() {
    let mut server = mockito::Server::new_async().await;
    mock_find_by_tags(&mut server, r#"{"secrets": [{"id": "sec-adopted"}]}"#)
        .create_async()
        .await;
    let create = mock_json(&mut server, "POST", SECRETS_PATH, r#"{"id": "sec-other"}"#)
        .expect(0)
        .create_async()
        .await;
    let version = mock_json(
        &mut server,
        "POST",
        &format!("{SECRETS_PATH}/sec-adopted/versions"),
        r#"{"revision": 4}"#,
    )
    .expect(1)
    .create_async()
    .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-adopt");
    let (source, _) = fixture.create_source_secret(&name, "password", true).await;
    fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let updated = fixture.get_scaleway_secret(&name).await;

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    create.assert_async().await;
    version.assert_async().await;
    let status = updated.status.expect("Expected status");
    assert_eq!(status.scaleway_id.as_deref(), Some("sec-adopted"));
    assert_eq!(status.current_version, Some(4));
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn test_scalewaysecret_rotation_survives_failed_disable_without_repush() {
    let mut server = mockito::Server::new_async().await;
    let version = mock_json(
        &mut server,
        "POST",
        &format!("{SECRETS_PATH}/sec-rot/versions"),
        r#"{"revision": 2}"#,
    )
    .expect(1)
    .create_async()
    .await;
    let disable = server
        .mock(
            "POST",
            format!("{SECRETS_PATH}/sec-rot/versions/1/disable").as_str(),
        )
        .with_status(500)
        .with_body("boom")
        .expect(1)
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-rot");
    let (source, source_rv) = fixture.create_source_secret(&name, "password", true).await;
    fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    fixture
        .set_scaleway_secret_status(&name, synced_status("sec-rot", 1, "stale-rv"))
        .await;
    let ctx = fixture.ctx(&server.url());

    let first = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let second = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let updated = fixture.get_scaleway_secret(&name).await;

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    assert!(first.is_ok(), "Expected Ok, got: {:?}", first);
    assert!(second.is_ok(), "Expected Ok, got: {:?}", second);
    // Une seule version créée malgré deux réconciliations et un disable en échec.
    version.assert_async().await;
    disable.assert_async().await;
    let status = updated.status.expect("Expected status");
    assert_eq!(status.current_version, Some(2));
    assert_eq!(status.last_synced_resource_version, Some(source_rv));
    assert_eq!(status.sync_state, "Synced");
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn test_scalewaysecret_opt_in_removed_disables_version_and_marks_revoked() {
    let mut server = mockito::Server::new_async().await;
    let disable = mock_json(
        &mut server,
        "POST",
        &format!("{SECRETS_PATH}/sec-rev/versions/3/disable"),
        "{}",
    )
    .expect(1)
    .create_async()
    .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-revoke");
    let (source, source_rv) = fixture.create_source_secret(&name, "password", false).await;
    fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    fixture
        .set_scaleway_secret_status(&name, synced_status("sec-rev", 3, &source_rv))
        .await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let updated = fixture.get_scaleway_secret(&name).await;

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    let err = result.expect_err("expected SecretOptInMissing");
    assert!(
        matches!(err, OperatorError::SecretOptInMissing(_)),
        "got: {err:?}"
    );
    disable.assert_async().await;
    let status = updated.status.expect("Expected status");
    assert_eq!(status.sync_state, "Revoked");
    assert_eq!(status.scaleway_id.as_deref(), Some("sec-rev"));
    assert_eq!(status.last_synced_resource_version, None);
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn test_scalewaysecret_failed_revocation_is_retried_and_not_marked_revoked() {
    let mut server = mockito::Server::new_async().await;
    let disable = server
        .mock(
            "POST",
            format!("{SECRETS_PATH}/sec-rev-ko/versions/3/disable").as_str(),
        )
        .with_status(500)
        .with_body("boom")
        .expect(1)
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-revoke-ko");
    let (source, source_rv) = fixture.create_source_secret(&name, "password", false).await;
    fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    fixture
        .set_scaleway_secret_status(&name, synced_status("sec-rev-ko", 3, &source_rv))
        .await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let updated = fixture.get_scaleway_secret(&name).await;

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    // L'erreur renvoyée est celle du disable, transitoire : la révocation sera retentée.
    let err = result.expect_err("expected the disable error");
    assert!(
        matches!(err, OperatorError::ScalewayError { .. }),
        "got: {err:?}"
    );
    assert!(!err.is_permanent_error());
    disable.assert_async().await;
    let status = updated.status.expect("Expected status");
    assert_ne!(status.sync_state, "Revoked");
    assert_eq!(status.last_synced_resource_version, Some(source_rv));
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn test_scalewaysecret_viewer_role_cannot_sync() {
    let mut server = mockito::Server::new_async().await;
    let any_scaleway_call = server
        .mock("POST", mockito::Matcher::Any)
        .expect(0)
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_VIEWER).await;
    let name = unique_name("scw-secret-viewer");
    let (source, _) = fixture.create_source_secret(&name, "password", true).await;
    fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let updated = fixture.get_scaleway_secret(&name).await;

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    let err = result.expect_err("expected a read-only role error");
    assert!(matches!(err, OperatorError::ConfigError(_)), "got: {err:?}");
    assert!(err.to_string().contains("read-only"), "got: {err}");
    any_scaleway_call.assert_async().await;
    let status = updated.status.expect("Expected status");
    assert_eq!(status.sync_state, "Error");
    assert_eq!(status.scaleway_id, None);
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn test_scalewaysecret_invalid_region_is_rejected_before_any_scaleway_call() {
    let mut server = mockito::Server::new_async().await;
    let any_get = server
        .mock("GET", mockito::Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    let any_post = server
        .mock("POST", mockito::Matcher::Any)
        .expect(0)
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-region");
    let (source, _) = fixture.create_source_secret(&name, "password", true).await;
    fixture
        .create_scaleway_secret_in_region(
            &name,
            &source,
            "password",
            "../../../instance/v1/zones/fr-par-1/servers/x?",
        )
        .await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let updated = fixture.get_scaleway_secret(&name).await;

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    let err = result.expect_err("expected an invalid region error");
    assert!(matches!(err, OperatorError::ConfigError(_)), "got: {err:?}");
    any_get.assert_async().await;
    any_post.assert_async().await;
    assert_eq!(updated.status.expect("Expected status").sync_state, "Error");
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn test_scalewaysecret_deletion_calls_delete_api_and_removes_finalizer() {
    let mut server = mockito::Server::new_async().await;
    let delete = server
        .mock("DELETE", format!("{SECRETS_PATH}/sec-del").as_str())
        .with_status(204)
        .expect(1)
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-delete");
    let (source, source_rv) = fixture.create_source_secret(&name, "password", true).await;
    fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    fixture
        .set_scaleway_secret_status(&name, synced_status("sec-del", 1, &source_rv))
        .await;
    let api: Api<ScalewaySecret> = Api::namespaced(fixture.client.clone(), NS_EDITOR);
    // Le finalizer retient l'objet : il reste lisible avec un deletionTimestamp.
    api.delete(&name, &DeleteParams::default())
        .await
        .expect("delete ScalewaySecret");
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let after = api.get_opt(&name).await.expect("get_opt ScalewaySecret");

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    delete.assert_async().await;
    assert!(
        after.is_none(),
        "the CR should be gone once its finalizer is removed"
    );
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn test_scalewaysecret_finalizer_added_on_first_reconcile() {
    let server = mockito::Server::new_async().await;
    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-finalizer");
    let (source, _) = fixture.create_source_secret(&name, "password", true).await;
    fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    let api: Api<ScalewaySecret> = Api::namespaced(fixture.client.clone(), NS_EDITOR);
    let remove_finalizer = serde_json::json!({ "metadata": { "finalizers": null } });
    api.patch(
        &name,
        &PatchParams::default(),
        &Patch::Merge(remove_finalizer),
    )
    .await
    .expect("remove finalizer");
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let updated = fixture.get_scaleway_secret(&name).await;

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    assert_eq!(
        updated.metadata.finalizers,
        Some(vec![SECRET_FINALIZER.to_string()])
    );
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn test_scalewaysecret_missing_source_secret_is_transient() {
    let server = mockito::Server::new_async().await;
    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-nosrc");
    fixture
        .create_scaleway_secret(&name, "does-not-exist", "password")
        .await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let updated = fixture.get_scaleway_secret(&name).await;

    fixture.cleanup_scaleway_secret(&name).await;

    let err = result.expect_err("expected SecretNotFound");
    assert!(
        matches!(err, OperatorError::SecretNotFound(_)),
        "got: {err:?}"
    );
    assert!(!err.is_permanent_error());
    let status = updated.status.expect("Expected status");
    assert_eq!(status.sync_state, "Error");
    // Le status ne doit pas révéler le nom du Secret recherché.
    assert!(!status
        .error_message
        .unwrap_or_default()
        .contains("does-not-exist"));
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn test_scalewaysecret_version_failure_sets_error_and_keeps_scaleway_id() {
    let mut server = mockito::Server::new_async().await;
    mock_find_by_tags(&mut server, r#"{"secrets": []}"#)
        .create_async()
        .await;
    mock_json(&mut server, "POST", SECRETS_PATH, r#"{"id": "sec-vko"}"#)
        .create_async()
        .await;
    server
        .mock("POST", format!("{SECRETS_PATH}/sec-vko/versions").as_str())
        .with_status(500)
        .with_body("boom")
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-vko");
    let (source, _) = fixture.create_source_secret(&name, "password", true).await;
    fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let updated = fixture.get_scaleway_secret(&name).await;

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    let err = result.expect_err("expected the version creation error");
    assert!(
        matches!(err, OperatorError::ScalewayError { .. }),
        "got: {err:?}"
    );
    let status = updated.status.expect("Expected status");
    assert_eq!(status.sync_state, "Error");
    // Le secret créé reste référencé : le prochain tour pousse une version sans le recréer.
    assert_eq!(status.scaleway_id.as_deref(), Some("sec-vko"));
    assert_eq!(status.current_version, None);
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn test_scalewaysecret_failed_deletion_keeps_finalizer() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("DELETE", format!("{SECRETS_PATH}/sec-del-ko").as_str())
        .with_status(500)
        .with_body("boom")
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-delete-ko");
    let (source, source_rv) = fixture.create_source_secret(&name, "password", true).await;
    fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    fixture
        .set_scaleway_secret_status(&name, synced_status("sec-del-ko", 1, &source_rv))
        .await;
    let api: Api<ScalewaySecret> = Api::namespaced(fixture.client.clone(), NS_EDITOR);
    api.delete(&name, &DeleteParams::default())
        .await
        .expect("delete ScalewaySecret");
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let after = api.get_opt(&name).await.expect("get_opt ScalewaySecret");

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    assert!(result.is_err(), "Expected Err, got: {:?}", result);
    let finalizers = after
        .expect("the CR must survive a failed Scaleway deletion")
        .metadata
        .finalizers;
    assert_eq!(finalizers, Some(vec![SECRET_FINALIZER.to_string()]));
}
