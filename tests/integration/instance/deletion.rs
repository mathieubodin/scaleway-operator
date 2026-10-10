//! Instance : finalizer et suppression.

use super::*;
use std::sync::Arc;

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn finalizer_added_on_first_reconcile() {
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
async fn deletion_with_scaleway_id_calls_delete_api() {
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
async fn deletion_without_scaleway_id_removes_finalizer_only() {
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
