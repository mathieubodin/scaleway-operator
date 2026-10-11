use super::*;

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn viewer_role_cannot_create() {
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

    let fixture = TestFixture::for_namespace(NS_VIEWER).await;
    let name = unique_name("rdb-viewer");
    fixture.create_rdb(&name, rdb_spec("unused")).await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_rdb(&name, &ctx).await;
    let updated = fixture.get_rdb(&name).await;

    fixture.cleanup_rdb(&name).await;

    let err = result.expect_err("expected a read-only role error");
    assert!(matches!(err, OperatorError::ConfigError(_)), "got: {err:?}");
    assert!(err.to_string().contains("read-only"), "got: {err}");
    any_get.assert_async().await;
    any_post.assert_async().await;
    let status = updated.status.expect("Expected status");
    assert_eq!(status.sync_state, "Error");
    assert_eq!(status.scaleway_id, None);
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn region_and_admin_user_changes_are_refused_by_the_api_server() {
    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("rdb-immutable");
    fixture.create_rdb(&name, rdb_spec("unused")).await;

    let region = fixture
        .patch_rdb_spec(&name, serde_json::json!({ "region": "nl-ams" }))
        .await;
    let admin_user = fixture
        .patch_rdb_spec(&name, serde_json::json!({ "admin_user": "root" }))
        .await;
    // Le moteur reste modifiable, pour suivre une montée de version faite côté Scaleway.
    let engine = fixture
        .patch_rdb_spec(&name, serde_json::json!({ "engine": "PostgreSQL-17" }))
        .await;
    let updated = fixture.get_rdb(&name).await;

    fixture.cleanup_rdb(&name).await;

    let err = region.expect_err("expected the region change to be refused");
    assert!(
        err.to_string().contains("spec.region is immutable"),
        "got: {err}"
    );
    let err = admin_user.expect_err("expected the admin_user change to be refused");
    assert!(
        err.to_string().contains("spec.admin_user is immutable"),
        "got: {err}"
    );
    assert!(engine.is_ok(), "Expected Ok, got: {:?}", engine.err());
    assert_eq!(updated.spec.region, "fr-par");
    assert_eq!(updated.spec.admin_user, "admin");
    assert_eq!(updated.spec.engine, "PostgreSQL-17");
}
