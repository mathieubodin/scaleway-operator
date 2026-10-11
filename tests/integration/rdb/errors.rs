use super::*;

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn missing_scaleway_secret_keeps_the_instance_pending() {
    let mut server = mockito::Server::new_async().await;
    mock_find_by_tags(&mut server, r#"{"instances": [], "total_count": 0}"#)
        .create_async()
        .await;
    let any_post = server
        .mock("POST", mockito::Matcher::Any)
        .expect(0)
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("rdb-nosecret");
    fixture.create_rdb(&name, rdb_spec("missing-secret")).await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_rdb(&name, &ctx).await;
    let updated = fixture.get_rdb(&name).await;

    fixture.cleanup_rdb(&name).await;

    let err = result.expect_err("expected ScalewaySecretNotReady");
    assert!(
        matches!(err, OperatorError::ScalewaySecretNotReady(_)),
        "got: {err:?}"
    );
    // C'est ce délai qui fait revenir le controller sans événement sur le CR.
    assert!(err.recheck_interval().is_some());
    any_post.assert_async().await;
    let status = updated.status.expect("Expected status");
    assert_eq!(status.sync_state, "Pending");
    assert!(status.error_message.is_some());
    assert_eq!(status.scaleway_id, None);
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn unsynced_scaleway_secret_keeps_the_instance_pending() {
    let mut server = mockito::Server::new_async().await;
    mock_find_by_tags(&mut server, r#"{"instances": [], "total_count": 0}"#)
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
    let name = unique_name("rdb-unsynced");
    let secret = format!("{name}-pwd");
    // ScalewaySecret créé mais jamais réconcilié : pas de status.
    fixture
        .create_scaleway_secret(&secret, "unused-source", "password")
        .await;
    fixture.create_rdb(&name, rdb_spec(&secret)).await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_rdb(&name, &ctx).await;
    let updated = fixture.get_rdb(&name).await;

    fixture.cleanup_rdb(&name).await;
    fixture.cleanup_scaleway_secret(&secret).await;

    let err = result.expect_err("expected ScalewaySecretNotReady");
    assert!(
        matches!(err, OperatorError::ScalewaySecretNotReady(_)),
        "got: {err:?}"
    );
    secret_manager.assert_async().await;
    any_post.assert_async().await;
    assert_eq!(
        updated.status.expect("Expected status").sync_state,
        "Pending"
    );
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn creation_failure_sets_error() {
    let mut server = mockito::Server::new_async().await;
    mock_find_by_tags(&mut server, r#"{"instances": [], "total_count": 0}"#)
        .create_async()
        .await;
    mock_access_secret(&mut server, "sec-pwd", 1)
        .create_async()
        .await;
    server
        .mock("POST", RDB_PATH)
        .with_status(400)
        .with_body(r#"{"message": "password does not respect the policy"}"#)
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("rdb-createko");
    let secret = fixture
        .create_synced_password_secret(&name, "sec-pwd", 1)
        .await;
    fixture.create_rdb(&name, rdb_spec(&secret)).await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_rdb(&name, &ctx).await;
    let updated = fixture.get_rdb(&name).await;

    fixture.cleanup_rdb(&name).await;
    fixture.cleanup_scaleway_secret(&secret).await;

    let err = result.expect_err("expected the creation error");
    assert!(
        matches!(err, OperatorError::ScalewayError { .. }),
        "got: {err:?}"
    );
    let status = updated.status.expect("Expected status");
    assert_eq!(status.sync_state, "Error");
    assert_eq!(status.scaleway_id, None);
    let message = status.error_message.expect("an error message");
    assert!(
        message.contains("password does not respect"),
        "got: {message}"
    );
    // Le mot de passe ne doit jamais remonter dans le status.
    assert!(!message.contains("S3cret!pass"), "got: {message}");
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn sync_failure_sets_error_and_keeps_the_instance() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", format!("{RDB_PATH}/rdb-syncko").as_str())
        .with_status(500)
        .with_body("boom")
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("rdb-syncko");
    fixture.create_rdb(&name, rdb_spec("unused")).await;
    fixture
        .set_rdb_status(&name, known_instance_status("rdb-syncko"))
        .await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_rdb(&name, &ctx).await;
    let updated = fixture.get_rdb(&name).await;

    fixture.cleanup_rdb(&name).await;

    assert!(result.is_err(), "Expected Err, got: {:?}", result);
    let status = updated.status.expect("Expected status");
    assert_eq!(status.sync_state, "Error");
    assert_eq!(status.scaleway_id.as_deref(), Some("rdb-syncko"));
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn reserved_tag_is_rejected_before_any_scaleway_call() {
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
    let name = unique_name("rdb-tags");
    let mut spec = rdb_spec("unused");
    spec.tags = vec!["scaleway-operator-cr-name=other-cr".to_string()];
    fixture.create_rdb(&name, spec).await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_rdb(&name, &ctx).await;
    let updated = fixture.get_rdb(&name).await;

    fixture.cleanup_rdb(&name).await;

    let err = result.expect_err("expected a reserved tag error");
    assert!(matches!(err, OperatorError::ConfigError(_)), "got: {err:?}");
    any_get.assert_async().await;
    any_post.assert_async().await;
    assert_eq!(updated.status.expect("Expected status").sync_state, "Error");
}
