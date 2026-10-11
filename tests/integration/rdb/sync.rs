use super::*;
use k8s_openapi::api::events::v1::Event;
use kube::api::ListParams;

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn create_reads_the_password_and_records_the_instance() {
    let mut server = mockito::Server::new_async().await;
    mock_find_by_tags(&mut server, r#"{"instances": [], "total_count": 0}"#)
        .create_async()
        .await;
    let access = mock_access_secret(&mut server, "sec-pwd", 3)
        .expect(1)
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("rdb-create");
    let create = server
        .mock("POST", RDB_PATH)
        .match_body(mockito::Matcher::PartialJson(serde_json::json!({
            "name": name,
            "engine": "PostgreSQL-16",
            "node_type": "DB-DEV-S",
            "user_name": "admin",
            "password": "S3cret!pass",
            "volume_type": "bssd",
            "volume_size": 20_000_000_000u64,
            "tags": [
                "managed-by=scaleway-operator",
                format!("scaleway-operator-cr-namespace={NS_EDITOR}"),
                format!("scaleway-operator-cr-name={name}"),
            ],
        })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"id": "rdb-new"}"#)
        .expect(1)
        .create_async()
        .await;
    let secret = fixture
        .create_synced_password_secret(&name, "sec-pwd", 3)
        .await;
    fixture.create_rdb(&name, rdb_spec(&secret)).await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_rdb(&name, &ctx).await;
    let updated = fixture.get_rdb(&name).await;

    fixture.cleanup_rdb(&name).await;
    fixture.cleanup_scaleway_secret(&secret).await;

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    access.assert_async().await;
    create.assert_async().await;
    let status = updated.status.expect("Expected status");
    assert_eq!(status.scaleway_id.as_deref(), Some("rdb-new"));
    assert_eq!(status.sync_state, "Syncing");
    assert_eq!(status.state, "provisioning");
    assert_eq!(status.error_message, None);
    assert!(status.project_id.is_some());
    assert!(status.created_at.is_some());
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn adoption_neither_reads_the_password_nor_creates() {
    let mut server = mockito::Server::new_async().await;
    mock_find_by_tags(
        &mut server,
        r#"{"instances": [{"id": "rdb-orphan"}], "total_count": 1}"#,
    )
    .create_async()
    .await;
    let secret_manager = server
        .mock("GET", mockito::Matcher::Regex("^/secret-manager/".into()))
        .expect(0)
        .create_async()
        .await;
    let any_post = server
        .mock("POST", mockito::Matcher::Any)
        .expect(0)
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("rdb-adopt");
    // Aucun ScalewaySecret : une instance adoptée a déjà son mot de passe.
    fixture.create_rdb(&name, rdb_spec("missing-secret")).await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_rdb(&name, &ctx).await;
    let updated = fixture.get_rdb(&name).await;

    fixture.cleanup_rdb(&name).await;

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    secret_manager.assert_async().await;
    any_post.assert_async().await;
    let status = updated.status.expect("Expected status");
    assert_eq!(status.scaleway_id.as_deref(), Some("rdb-orphan"));
    assert_eq!(status.sync_state, "Syncing");
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn stale_cached_cr_does_not_create_a_second_instance() {
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
    let name = unique_name("rdb-stale");
    // Ce que voit le cache du controller : aucune instance connue.
    let cached = fixture.create_rdb(&name, rdb_spec("unused")).await;
    // Ce que porte déjà l'API server : la création s'est terminée.
    fixture
        .set_rdb_status(&name, known_instance_status("rdb-done"))
        .await;
    let ctx = fixture.ctx(&server.url());

    let result = scaleway_operator::reconcilers::reconcile_rdb(Arc::new(cached), ctx.clone()).await;
    let updated = fixture.get_rdb(&name).await;

    fixture.cleanup_rdb(&name).await;

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    any_get.assert_async().await;
    any_post.assert_async().await;
    let status = updated.status.expect("Expected status");
    assert_eq!(status.scaleway_id.as_deref(), Some("rdb-done"));
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn ready_instance_is_synced_with_its_endpoint() {
    let mut server = mockito::Server::new_async().await;
    mock_json(
        &mut server,
        "GET",
        &format!("{RDB_PATH}/rdb-ready"),
        &instance_body("rdb-ready", "ready"),
    )
    .create_async()
    .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("rdb-ready");
    fixture.create_rdb(&name, rdb_spec("unused")).await;
    fixture
        .set_rdb_status(&name, known_instance_status("rdb-ready"))
        .await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_rdb(&name, &ctx).await;
    let updated = fixture.get_rdb(&name).await;

    fixture.cleanup_rdb(&name).await;

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    let status = updated.status.expect("Expected status");
    assert_eq!(status.sync_state, "Synced");
    assert_eq!(status.state, "ready");
    assert_eq!(status.endpoint_ip.as_deref(), Some("51.158.1.2"));
    assert_eq!(status.endpoint_port, Some(5432));
    assert_eq!(status.engine.as_deref(), Some("PostgreSQL-16"));
    assert_eq!(status.node_type.as_deref(), Some("DB-DEV-S"));
    assert_eq!(status.drift_message, None);
    assert_eq!(status.error_message, None);
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn mutable_drift_is_reported_once_in_status_and_as_an_event() {
    let mut server = mockito::Server::new_async().await;
    let mut body: serde_json::Value =
        serde_json::from_str(&instance_body("rdb-drift", "ready")).unwrap();
    body["node_type"] = "DB-DEV-M".into();
    mock_json(
        &mut server,
        "GET",
        &format!("{RDB_PATH}/rdb-drift"),
        &body.to_string(),
    )
    .create_async()
    .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("rdb-drift");
    fixture.create_rdb(&name, rdb_spec("unused")).await;
    fixture
        .set_rdb_status(&name, known_instance_status("rdb-drift"))
        .await;
    let ctx = fixture.ctx(&server.url());

    let first = fixture.reconcile_rdb(&name, &ctx).await;
    // La dérive dure : le second passage ne doit rien émettre de plus.
    let second = fixture.reconcile_rdb(&name, &ctx).await;
    let updated = fixture.get_rdb(&name).await;
    let events: Api<Event> = Api::namespaced(fixture.client.clone(), NS_EDITOR);
    let drift_events: Vec<Event> = events
        .list(&ListParams::default().fields(&format!("regarding.name={name}")))
        .await
        .expect("list events")
        .items
        .into_iter()
        .filter(|e| e.reason.as_deref() == Some("SpecDrift"))
        .collect();

    fixture.cleanup_rdb(&name).await;

    assert!(first.is_ok(), "Expected Ok, got: {:?}", first);
    assert!(second.is_ok(), "Expected Ok, got: {:?}", second);
    let status = updated.status.expect("Expected status");
    // Une dérive modifiable est signalée sans bloquer la synchro.
    assert_eq!(status.sync_state, "Synced");
    assert_eq!(
        status.drift_message.as_deref(),
        Some("node_type: spec 'DB-DEV-S', Scaleway 'DB-DEV-M'")
    );
    assert_eq!(drift_events.len(), 1, "got: {drift_events:?}");
    assert_eq!(drift_events[0].type_.as_deref(), Some("Warning"));
    assert_eq!(drift_events[0].note, status.drift_message);
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn engine_drift_is_config_drift() {
    let mut server = mockito::Server::new_async().await;
    let mut body: serde_json::Value =
        serde_json::from_str(&instance_body("rdb-engine", "ready")).unwrap();
    body["engine"] = "PostgreSQL-17".into();
    mock_json(
        &mut server,
        "GET",
        &format!("{RDB_PATH}/rdb-engine"),
        &body.to_string(),
    )
    .create_async()
    .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("rdb-engine");
    fixture.create_rdb(&name, rdb_spec("unused")).await;
    fixture
        .set_rdb_status(&name, known_instance_status("rdb-engine"))
        .await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_rdb(&name, &ctx).await;
    let updated = fixture.get_rdb(&name).await;

    fixture.cleanup_rdb(&name).await;

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    let status = updated.status.expect("Expected status");
    assert_eq!(status.sync_state, "ConfigDrift");
    assert!(
        status
            .error_message
            .as_deref()
            .is_some_and(|m| m.contains("PostgreSQL-17")),
        "got: {:?}",
        status.error_message
    );
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn instance_gone_from_scaleway_is_forgotten_for_recreation() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", format!("{RDB_PATH}/rdb-gone").as_str())
        .with_status(404)
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("rdb-gone");
    fixture.create_rdb(&name, rdb_spec("unused")).await;
    fixture
        .set_rdb_status(&name, known_instance_status("rdb-gone"))
        .await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_rdb(&name, &ctx).await;
    let updated = fixture.get_rdb(&name).await;

    fixture.cleanup_rdb(&name).await;

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    let status = updated.status.expect("Expected status");
    assert_eq!(status.scaleway_id, None);
    assert_eq!(status.sync_state, "Pending");
}
