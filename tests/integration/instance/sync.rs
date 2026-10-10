//! Instance : création, adoption et synchronisation.

use super::*;
use std::sync::Arc;

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn create_writes_scaleway_id_to_status() {
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
async fn orphan_adoption_does_not_call_create() {
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
async fn sync_updates_state_and_public_ip() {
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
