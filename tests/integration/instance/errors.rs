//! Instance : prérequis Kubernetes manquants et erreurs de l'API Scaleway.

use super::*;
use std::sync::Arc;

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn missing_namespace_role_returns_config_error() {
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
async fn missing_project_id_annotation_returns_config_error() {
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
async fn invalid_uuid_annotation_returns_config_error() {
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
async fn missing_iam_secret_returns_config_error() {
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
async fn scaleway_error_sets_sync_state_error() {
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
