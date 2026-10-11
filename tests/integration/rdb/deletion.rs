use super::*;

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn finalizer_added_on_first_reconcile() {
    let server = mockito::Server::new_async().await;
    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("rdb-finalizer");
    fixture.create_rdb(&name, rdb_spec("unused")).await;
    let api: Api<Rdb> = Api::namespaced(fixture.client.clone(), NS_EDITOR);
    let no_finalizer = serde_json::json!({ "metadata": { "finalizers": null } });
    api.patch(&name, &PatchParams::default(), &Patch::Merge(no_finalizer))
        .await
        .expect("remove finalizer");
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_rdb(&name, &ctx).await;
    let updated = fixture.get_rdb(&name).await;

    fixture.cleanup_rdb(&name).await;

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    assert_eq!(
        updated.metadata.finalizers,
        Some(vec![RDB_FINALIZER.to_string()])
    );
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn deletion_deletes_the_instance_and_removes_the_finalizer() {
    let mut server = mockito::Server::new_async().await;
    let delete = server
        .mock("DELETE", format!("{RDB_PATH}/rdb-del").as_str())
        .with_status(200)
        .with_body("{}")
        .expect(1)
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("rdb-delete");
    fixture.create_rdb(&name, rdb_spec("unused")).await;
    fixture
        .set_rdb_status(&name, known_instance_status("rdb-del"))
        .await;
    fixture.request_rdb_deletion(&name).await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_rdb(&name, &ctx).await;
    let still_there = fixture.rdb_exists(&name).await;

    fixture.cleanup_rdb(&name).await;

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    delete.assert_async().await;
    assert!(
        !still_there,
        "the CR should be gone once its finalizer is removed"
    );
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn deletion_with_retain_leaves_the_instance_in_scaleway() {
    let mut server = mockito::Server::new_async().await;
    let any_delete = server
        .mock("DELETE", mockito::Matcher::Any)
        .expect(0)
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("rdb-retain");
    let mut spec = rdb_spec("unused");
    spec.deletion_policy = DeletionPolicy::Retain;
    fixture.create_rdb(&name, spec).await;
    fixture
        .set_rdb_status(&name, known_instance_status("rdb-kept"))
        .await;
    fixture.request_rdb_deletion(&name).await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_rdb(&name, &ctx).await;
    let still_there = fixture.rdb_exists(&name).await;

    fixture.cleanup_rdb(&name).await;

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    any_delete.assert_async().await;
    assert!(!still_there, "the CR should be gone, the instance kept");
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn refused_deletion_keeps_the_finalizer_and_marks_deleting() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("DELETE", format!("{RDB_PATH}/rdb-busy").as_str())
        .with_status(409)
        .with_body(r#"{"message": "resource is in a transient state"}"#)
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("rdb-busy");
    fixture.create_rdb(&name, rdb_spec("unused")).await;
    fixture
        .set_rdb_status(&name, known_instance_status("rdb-busy"))
        .await;
    fixture.request_rdb_deletion(&name).await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_rdb(&name, &ctx).await;
    let still_there = fixture.rdb_exists(&name).await;
    let updated = if still_there {
        Some(fixture.get_rdb(&name).await)
    } else {
        None
    };

    fixture.cleanup_rdb(&name).await;

    // Requeue à délai fixe, sans erreur : Scaleway finira par accepter.
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    let updated = updated.expect("the CR must survive a refused deletion");
    assert_eq!(
        updated.metadata.finalizers,
        Some(vec![RDB_FINALIZER.to_string()])
    );
    let status = updated.status.expect("Expected status");
    assert_eq!(status.sync_state, "Deleting");
    assert_eq!(status.scaleway_id.as_deref(), Some("rdb-busy"));
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn failed_deletion_keeps_the_finalizer() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("DELETE", format!("{RDB_PATH}/rdb-outage").as_str())
        .with_status(503)
        .with_body("boom")
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("rdb-outage");
    fixture.create_rdb(&name, rdb_spec("unused")).await;
    fixture
        .set_rdb_status(&name, known_instance_status("rdb-outage"))
        .await;
    fixture.request_rdb_deletion(&name).await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_rdb(&name, &ctx).await;
    let still_there = fixture.rdb_exists(&name).await;

    fixture.cleanup_rdb(&name).await;

    assert!(result.is_err(), "Expected Err, got: {:?}", result);
    assert!(still_there, "the CR must survive a failed deletion");
}
