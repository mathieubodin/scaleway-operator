use k8s_openapi::api::core::v1::{Namespace, Secret};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{DeleteParams, PatchParams, PostParams};
use kube::{Api, Client};
use scaleway_operator::{
    context::Context,
    resources::{Instance, InstanceSpec, NamespaceRole, NamespaceRoleSpec},
    scaleway::ScalewayClient,
};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Crée un kube::Client en respectant la variable d'environnement KUBE_API_URL.
/// Quand kubectl proxy tourne sur localhost:8001, définir :
///   KUBE_API_URL=http://127.0.0.1:8001
/// Sans cette variable, Client::try_default() utilise le kubeconfig courant.
async fn make_client() -> Client {
    match std::env::var("KUBE_API_URL") {
        Ok(url) => {
            let config = kube::Config::new(
                url.parse()
                    .unwrap_or_else(|_| panic!("KUBE_API_URL is not a valid URI: {}", url)),
            );
            Client::try_from(config).expect("Failed to build client from KUBE_API_URL")
        }
        Err(_) => Client::try_default()
            .await
            .expect("Cannot connect to Kubernetes — set KUBE_API_URL=http://127.0.0.1:8001 when using kubectl proxy, or ensure kubeconfig points to a reachable cluster"),
    }
}

const TEST_PROJECT_ID: &str = "11111111-1111-1111-1111-111111111111";
const SCALEWAY_SYSTEM_NS: &str = "scaleway-system";
const INSTANCE_FINALIZER: &str = "scaleway.io/instance-finalizer";

// ---------------------------------------------------------------------------
// TestFixture — gestion du cycle de vie des ressources k8s par test
// ---------------------------------------------------------------------------

pub struct TestFixture {
    pub client: Client,
    pub ns: String,
}

impl TestFixture {
    pub async fn new() -> Self {
        let client = make_client().await;

        let ns = format!("test-scw-{}", &uuid::Uuid::new_v4().to_string()[..8]);

        // Créer le namespace avec annotation project-id et label de cleanup
        let ns_api: Api<Namespace> = Api::all(client.clone());
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "scaleway.io/project-id".to_string(),
            TEST_PROJECT_ID.to_string(),
        );
        let mut labels = BTreeMap::new();
        labels.insert(
            "app.kubernetes.io/managed-by".to_string(),
            "scaleway-operator-test".to_string(),
        );

        let namespace_obj = Namespace {
            metadata: ObjectMeta {
                name: Some(ns.clone()),
                annotations: Some(annotations),
                labels: Some(labels),
                ..Default::default()
            },
            ..Default::default()
        };
        ns_api
            .create(&PostParams::default(), &namespace_obj)
            .await
            .expect("Failed to create test namespace");

        TestFixture { client, ns }
    }

    /// Crée le namespace sans l'annotation scaleway.io/project-id.
    pub async fn new_without_annotation() -> Self {
        let client = make_client().await;

        let ns = format!("test-scw-{}", &uuid::Uuid::new_v4().to_string()[..8]);

        let ns_api: Api<Namespace> = Api::all(client.clone());
        let mut labels = BTreeMap::new();
        labels.insert(
            "app.kubernetes.io/managed-by".to_string(),
            "scaleway-operator-test".to_string(),
        );

        let namespace_obj = Namespace {
            metadata: ObjectMeta {
                name: Some(ns.clone()),
                labels: Some(labels),
                ..Default::default()
            },
            ..Default::default()
        };
        ns_api
            .create(&PostParams::default(), &namespace_obj)
            .await
            .expect("Failed to create test namespace");

        TestFixture { client, ns }
    }

    /// Crée le namespace avec une annotation project-id dont la valeur n'est pas un UUID.
    pub async fn new_with_invalid_annotation() -> Self {
        let client = make_client().await;

        let ns = format!("test-scw-{}", &uuid::Uuid::new_v4().to_string()[..8]);

        let ns_api: Api<Namespace> = Api::all(client.clone());
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "scaleway.io/project-id".to_string(),
            "not-a-uuid".to_string(),
        );
        let mut labels = BTreeMap::new();
        labels.insert(
            "app.kubernetes.io/managed-by".to_string(),
            "scaleway-operator-test".to_string(),
        );

        let namespace_obj = Namespace {
            metadata: ObjectMeta {
                name: Some(ns.clone()),
                annotations: Some(annotations),
                labels: Some(labels),
                ..Default::default()
            },
            ..Default::default()
        };
        ns_api
            .create(&PostParams::default(), &namespace_obj)
            .await
            .expect("Failed to create test namespace");

        TestFixture { client, ns }
    }

    /// Crée une ressource NamespaceRole cluster-wide pour ce namespace.
    pub async fn setup_namespace_role(&self, role: &str) {
        let api: Api<NamespaceRole> = Api::all(self.client.clone());
        let nr = NamespaceRole {
            metadata: ObjectMeta {
                name: Some(self.ns.clone()),
                ..Default::default()
            },
            spec: NamespaceRoleSpec {
                namespace: self.ns.clone(),
                scaleway_role: role.to_string(),
                description: None,
            },
            status: None,
        };
        api.create(&PostParams::default(), &nr)
            .await
            .expect("Failed to create NamespaceRole");
    }

    /// Crée le namespace scaleway-system et le Secret IAM via ServerSideApply (idempotent).
    pub async fn setup_iam_secret(&self) {
        // S'assurer que scaleway-system existe (idempotent via SSA)
        let ns_api: Api<Namespace> = Api::all(self.client.clone());
        let system_ns = Namespace {
            metadata: ObjectMeta {
                name: Some(SCALEWAY_SYSTEM_NS.to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        let ssapply = PatchParams::apply("integration-test").force();
        ns_api
            .patch(
                SCALEWAY_SYSTEM_NS,
                &ssapply,
                &kube::api::Patch::Apply(system_ns),
            )
            .await
            .expect("Failed to create/patch scaleway-system namespace");

        // Créer le Secret avec le champ secret_key (requis par get_namespace_client)
        let secret_name = format!("scaleway-ns-creds-{}", self.ns);
        let secrets_api: Api<Secret> = Api::namespaced(self.client.clone(), SCALEWAY_SYSTEM_NS);
        let mut data = BTreeMap::new();
        data.insert(
            "secret_key".to_string(),
            k8s_openapi::ByteString(b"mock-secret-key".to_vec()),
        );
        let secret = Secret {
            metadata: ObjectMeta {
                name: Some(secret_name.clone()),
                namespace: Some(SCALEWAY_SYSTEM_NS.to_string()),
                ..Default::default()
            },
            data: Some(data),
            ..Default::default()
        };
        secrets_api
            .patch(&secret_name, &ssapply, &kube::api::Patch::Apply(secret))
            .await
            .expect("Failed to create/patch IAM secret");
    }

    /// Construit un Arc<Context> avec les deux clients pointant vers le mock_url.
    pub fn ctx(&self, mock_url: &str) -> Arc<Context> {
        Arc::new(Context {
            client: self.client.clone(),
            scaleway_client: ScalewayClient::new_with_base_url(
                "test-token".to_string(),
                mock_url.to_string(),
            ),
            organization_id: "test-org".to_string(),
            scaleway_base_url: mock_url.to_string(),
        })
    }

    /// Nettoie toutes les ressources k8s créées par ce fixture.
    /// À appeler explicitement à la fin de chaque test.
    pub async fn cleanup(&self) {
        // Supprimer la NamespaceRole cluster-wide (si elle existe)
        let nr_api: Api<NamespaceRole> = Api::all(self.client.clone());
        let _ = nr_api.delete(&self.ns, &DeleteParams::default()).await;

        // Supprimer le Secret IAM dans scaleway-system (si il existe)
        let secret_name = format!("scaleway-ns-creds-{}", self.ns);
        let secrets_api: Api<Secret> = Api::namespaced(self.client.clone(), SCALEWAY_SYSTEM_NS);
        let _ = secrets_api
            .delete(&secret_name, &DeleteParams::default())
            .await;

        // Supprimer le namespace (cascade vers Instances namespaced)
        let ns_api: Api<Namespace> = Api::all(self.client.clone());
        let _ = ns_api.delete(&self.ns, &DeleteParams::default()).await;
    }
}

// ---------------------------------------------------------------------------
// Helpers de construction d'objets Instance
// ---------------------------------------------------------------------------

pub fn build_instance(ns: &str, name: &str) -> Instance {
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

pub fn build_instance_with_finalizer(ns: &str, name: &str) -> Instance {
    let mut instance = build_instance(ns, name);
    instance.metadata.finalizers = Some(vec![INSTANCE_FINALIZER.to_string()]);
    instance
}

// ---------------------------------------------------------------------------
// Tests d'intégration — U4 : prérequis Kubernetes manquants
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires local Kubernetes cluster — run: make deploy-crd && make test-integration"]
async fn test_missing_namespace_role_returns_config_error() {
    let server = mockito::Server::new_async().await;
    let url = server.url();
    // Pas de mock nécessaire — retour avant étape 5

    let fixture = TestFixture::new().await;
    // Intentionnellement pas de setup_namespace_role()
    let ctx = fixture.ctx(&url);
    let instance = Arc::new(build_instance(&fixture.ns, "test-instance"));

    let result = scaleway_operator::reconcilers::reconcile_instance(instance, ctx).await;

    fixture.cleanup().await;
    drop(server);

    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("No NamespaceRole found"),
        "Expected 'No NamespaceRole found', got: {}",
        err
    );
}

#[tokio::test]
#[ignore = "requires local Kubernetes cluster — run: make deploy-crd && make test-integration"]
async fn test_missing_project_id_annotation_returns_config_error() {
    let server = mockito::Server::new_async().await;
    let url = server.url();

    let fixture = TestFixture::new_without_annotation().await;
    fixture.setup_namespace_role("Editor").await;
    let ctx = fixture.ctx(&url);
    let instance = Arc::new(build_instance(&fixture.ns, "test-instance"));

    let result = scaleway_operator::reconcilers::reconcile_instance(instance, ctx).await;

    fixture.cleanup().await;
    drop(server);

    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("scaleway.io/project-id"),
        "Expected 'scaleway.io/project-id' in error, got: {}",
        err
    );
}

#[tokio::test]
#[ignore = "requires local Kubernetes cluster — run: make deploy-crd && make test-integration"]
async fn test_invalid_uuid_annotation_returns_config_error() {
    let server = mockito::Server::new_async().await;
    let url = server.url();

    let fixture = TestFixture::new_with_invalid_annotation().await;
    fixture.setup_namespace_role("Editor").await;
    let ctx = fixture.ctx(&url);
    let instance = Arc::new(build_instance(&fixture.ns, "test-instance"));

    let result = scaleway_operator::reconcilers::reconcile_instance(instance, ctx).await;

    fixture.cleanup().await;
    drop(server);

    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("must be a valid UUID"),
        "Expected 'must be a valid UUID' in error, got: {}",
        err
    );
}

#[tokio::test]
#[ignore = "requires local Kubernetes cluster — run: make deploy-crd && make test-integration"]
async fn test_missing_iam_secret_returns_config_error() {
    let server = mockito::Server::new_async().await;
    let url = server.url();
    // Pas de mock Scaleway — retour avant étape 6

    let fixture = TestFixture::new().await;
    fixture.setup_namespace_role("Editor").await;
    // Intentionnellement pas de setup_iam_secret()
    let ctx = fixture.ctx(&url);
    // Finalizer pré-présent requis pour atteindre l'étape 6 (sinon requeue à l'étape 4)
    let instance = Arc::new(build_instance_with_finalizer(&fixture.ns, "test-instance"));

    let result = scaleway_operator::reconcilers::reconcile_instance(instance, ctx).await;

    fixture.cleanup().await;
    drop(server);

    let err = result.unwrap_err();
    assert!(
        err.to_string()
            .contains("not found in namespace 'scaleway-system'"),
        "Expected secret not found error, got: {}",
        err
    );
}

#[tokio::test]
#[ignore = "requires local Kubernetes cluster — run: make deploy-crd && make test-integration"]
async fn test_viewer_role_cannot_create_instance() {
    let server = mockito::Server::new_async().await;
    let url = server.url();
    // Pas de mock Scaleway — retour à l'étape 8 avant verify_project_access

    let fixture = TestFixture::new().await;
    fixture.setup_namespace_role("Viewer").await;
    fixture.setup_iam_secret().await;
    let ctx = fixture.ctx(&url);
    // Finalizer pré-présent requis pour atteindre l'étape 8
    let instance = Arc::new(build_instance_with_finalizer(&fixture.ns, "test-instance"));

    let result = scaleway_operator::reconcilers::reconcile_instance(instance, ctx).await;

    fixture.cleanup().await;
    drop(server);

    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("read-only"),
        "Expected 'read-only' in error, got: {}",
        err
    );
}

// ---------------------------------------------------------------------------
// Tests d'intégration — U5 : lifecycle du finalizer
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires local Kubernetes cluster — run: make deploy-crd && make test-integration"]
async fn test_finalizer_added_on_first_reconcile() {
    // Aucun mock Scaleway requis — le réconciliateur retourne à l'étape 4
    let server = mockito::Server::new_async().await;
    let url = server.url();

    let fixture = TestFixture::new().await;
    fixture.setup_namespace_role("Editor").await;

    let instance_api: Api<Instance> = Api::namespaced(fixture.client.clone(), &fixture.ns);
    let instance_obj = build_instance(&fixture.ns, "test-finalizer");
    instance_api
        .create(&PostParams::default(), &instance_obj)
        .await
        .expect("Failed to create Instance in k8s");

    let fetched = instance_api
        .get("test-finalizer")
        .await
        .expect("Failed to fetch Instance");
    let ctx = fixture.ctx(&url);

    let result = scaleway_operator::reconcilers::reconcile_instance(Arc::new(fetched), ctx).await;

    // Vérifier que le finalizer est dans k8s
    let updated = instance_api
        .get("test-finalizer")
        .await
        .expect("Failed to fetch updated Instance");

    fixture.cleanup().await;
    drop(server);

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    let finalizers = updated.metadata.finalizers.unwrap_or_default();
    assert!(
        finalizers.contains(&INSTANCE_FINALIZER.to_string()),
        "Expected finalizer in k8s object, got: {:?}",
        finalizers
    );
}

#[tokio::test]
#[ignore = "requires local Kubernetes cluster — run: make deploy-crd && make test-integration"]
async fn test_deletion_with_scaleway_id_calls_delete_api() {
    // handle_deletion utilise ctx.scaleway_client — configurer le mock sur le même server
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    let mock_delete = server
        .mock("DELETE", "/instance/v1/zones/fr-par-1/servers/srv-test-123")
        .with_status(204)
        .with_body("")
        .create_async()
        .await;

    let fixture = TestFixture::new().await;
    // Pas besoin de NamespaceRole ni Secret — handle_deletion est à l'étape 1

    let instance_api: Api<Instance> = Api::namespaced(fixture.client.clone(), &fixture.ns);
    let mut instance_obj = build_instance_with_finalizer(&fixture.ns, "test-deletion");
    instance_obj.status = Some(scaleway_operator::resources::InstanceStatus {
        scaleway_id: Some("srv-test-123".to_string()),
        ..Default::default()
    });
    instance_api
        .create(&PostParams::default(), &instance_obj)
        .await
        .expect("Failed to create Instance in k8s");

    // Simuler la suppression : fetcher et cloner avec deletion_timestamp
    let mut fetched = instance_api
        .get("test-deletion")
        .await
        .expect("Failed to fetch Instance");
    fetched.metadata.deletion_timestamp = Some(
        k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(k8s_openapi::jiff::Timestamp::now()),
    );

    let ctx = fixture.ctx(&url);
    let result = scaleway_operator::reconcilers::reconcile_instance(Arc::new(fetched), ctx).await;

    // Vérifier que le finalizer a été retiré
    let updated = instance_api
        .get("test-deletion")
        .await
        .expect("Failed to fetch updated Instance");

    fixture.cleanup().await;
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
#[ignore = "requires local Kubernetes cluster — run: make deploy-crd && make test-integration"]
async fn test_deletion_without_scaleway_id_removes_finalizer_only() {
    let server = mockito::Server::new_async().await;
    let url = server.url();
    // Pas de mock — pas d'appel Scaleway quand scaleway_id est absent

    let fixture = TestFixture::new().await;

    let instance_api: Api<Instance> = Api::namespaced(fixture.client.clone(), &fixture.ns);
    let instance_obj = build_instance_with_finalizer(&fixture.ns, "test-noid-deletion");
    instance_api
        .create(&PostParams::default(), &instance_obj)
        .await
        .expect("Failed to create Instance in k8s");

    let mut fetched = instance_api
        .get("test-noid-deletion")
        .await
        .expect("Failed to fetch Instance");
    fetched.metadata.deletion_timestamp = Some(
        k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(k8s_openapi::jiff::Timestamp::now()),
    );

    let ctx = fixture.ctx(&url);
    let result = scaleway_operator::reconcilers::reconcile_instance(Arc::new(fetched), ctx).await;

    let updated = instance_api
        .get("test-noid-deletion")
        .await
        .expect("Failed to fetch updated Instance");

    fixture.cleanup().await;
    drop(server);

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    let finalizers = updated.metadata.finalizers.clone().unwrap_or_default();
    assert!(
        !finalizers.contains(&INSTANCE_FINALIZER.to_string()),
        "Expected finalizer removed without Scaleway call, got: {:?}",
        finalizers
    );
}

// ---------------------------------------------------------------------------
// Tests d'intégration — U6 : création et synchronisation d'instance
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires local Kubernetes cluster — run: make deploy-crd && make test-integration"]
async fn test_create_instance_writes_scaleway_id_to_status() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    // Mocks Scaleway pour le flux de création (étape 8)
    // verify_project_access
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
    // find_instance_by_name → liste vide
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
    // create_instance → 201 avec server.id
    let mock_create = server
        .mock("POST", "/instance/v1/zones/fr-par-1/servers")
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(r#"{"server": {"id": "srv-new-123"}}"#)
        .create_async()
        .await;

    let fixture = TestFixture::new().await;
    fixture.setup_namespace_role("Editor").await;
    fixture.setup_iam_secret().await;

    let instance_api: Api<Instance> = Api::namespaced(fixture.client.clone(), &fixture.ns);
    let instance_obj = build_instance_with_finalizer(&fixture.ns, "test-create");
    instance_api
        .create(&PostParams::default(), &instance_obj)
        .await
        .expect("Failed to create Instance in k8s");

    let fetched = instance_api
        .get("test-create")
        .await
        .expect("Failed to fetch Instance");
    let ctx = fixture.ctx(&url);

    let result = scaleway_operator::reconcilers::reconcile_instance(Arc::new(fetched), ctx).await;

    let updated = instance_api
        .get("test-create")
        .await
        .expect("Failed to fetch updated Instance");

    fixture.cleanup().await;
    mock_create.assert_async().await;
    drop(server);

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    let status = updated.status.expect("Expected status to be set");
    assert_eq!(
        status.scaleway_id,
        Some("srv-new-123".to_string()),
        "Expected scaleway_id to be set"
    );
    assert_eq!(status.sync_state, "Syncing");
}

#[tokio::test]
#[ignore = "requires local Kubernetes cluster — run: make deploy-crd && make test-integration"]
async fn test_orphan_adoption_does_not_call_create() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    // verify_project_access
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
    // find_instance_by_name → retourne une instance orpheline existante
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
    // create_instance ne doit PAS être appelé — pas de mock POST

    let fixture = TestFixture::new().await;
    fixture.setup_namespace_role("Editor").await;
    fixture.setup_iam_secret().await;

    let instance_api: Api<Instance> = Api::namespaced(fixture.client.clone(), &fixture.ns);
    let instance_obj = build_instance_with_finalizer(&fixture.ns, "test-orphan");
    instance_api
        .create(&PostParams::default(), &instance_obj)
        .await
        .expect("Failed to create Instance in k8s");

    let fetched = instance_api
        .get("test-orphan")
        .await
        .expect("Failed to fetch Instance");
    let ctx = fixture.ctx(&url);

    let result = scaleway_operator::reconcilers::reconcile_instance(Arc::new(fetched), ctx).await;

    let updated = instance_api
        .get("test-orphan")
        .await
        .expect("Failed to fetch updated Instance");

    fixture.cleanup().await;
    drop(server);

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    let status = updated.status.expect("Expected status to be set");
    assert_eq!(
        status.scaleway_id,
        Some("srv-orphan-456".to_string()),
        "Expected orphan instance to be adopted"
    );
}

#[tokio::test]
#[ignore = "requires local Kubernetes cluster — run: make deploy-crd && make test-integration"]
async fn test_sync_updates_state_and_public_ip() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    // get_instance via ns_client (même base_url via scaleway_base_url)
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

    let fixture = TestFixture::new().await;
    fixture.setup_namespace_role("Editor").await;
    fixture.setup_iam_secret().await;

    let instance_api: Api<Instance> = Api::namespaced(fixture.client.clone(), &fixture.ns);
    let mut instance_obj = build_instance_with_finalizer(&fixture.ns, "test-sync");
    instance_obj.status = Some(scaleway_operator::resources::InstanceStatus {
        scaleway_id: Some("srv-running-789".to_string()),
        project_id: Some(TEST_PROJECT_ID.to_string()),
        ..Default::default()
    });
    instance_api
        .create(&PostParams::default(), &instance_obj)
        .await
        .expect("Failed to create Instance in k8s");

    let fetched = instance_api
        .get("test-sync")
        .await
        .expect("Failed to fetch Instance");
    let ctx = fixture.ctx(&url);

    let result = scaleway_operator::reconcilers::reconcile_instance(Arc::new(fetched), ctx).await;

    let updated = instance_api
        .get("test-sync")
        .await
        .expect("Failed to fetch updated Instance");

    fixture.cleanup().await;
    drop(server);

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    let status = updated.status.expect("Expected status to be set");
    assert_eq!(status.state, "running");
    assert_eq!(status.public_ip, Some("1.2.3.4".to_string()));
    assert_eq!(status.sync_state, "Synced");
}

#[tokio::test]
#[ignore = "requires local Kubernetes cluster — run: make deploy-crd && make test-integration"]
async fn test_scaleway_error_sets_sync_state_error() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    // get_instance retourne 500
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

    let fixture = TestFixture::new().await;
    fixture.setup_namespace_role("Editor").await;
    fixture.setup_iam_secret().await;

    let instance_api: Api<Instance> = Api::namespaced(fixture.client.clone(), &fixture.ns);
    let mut instance_obj = build_instance_with_finalizer(&fixture.ns, "test-error");
    instance_obj.status = Some(scaleway_operator::resources::InstanceStatus {
        scaleway_id: Some("srv-error-000".to_string()),
        project_id: Some(TEST_PROJECT_ID.to_string()),
        ..Default::default()
    });
    instance_api
        .create(&PostParams::default(), &instance_obj)
        .await
        .expect("Failed to create Instance in k8s");

    let fetched = instance_api
        .get("test-error")
        .await
        .expect("Failed to fetch Instance");
    let ctx = fixture.ctx(&url);

    let result = scaleway_operator::reconcilers::reconcile_instance(Arc::new(fetched), ctx).await;

    let updated = instance_api
        .get("test-error")
        .await
        .expect("Failed to fetch updated Instance");

    fixture.cleanup().await;
    drop(server);

    assert!(result.is_err(), "Expected Err on Scaleway 500");
    let status = updated.status.expect("Expected status to be set");
    assert_eq!(status.sync_state, "Error");
}
