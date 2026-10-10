//! ScalewaySecret : spec invalide, Secret source ou clé absents, erreurs de l'API Scaleway.

use super::*;

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn invalid_region_is_rejected_before_any_scaleway_call() {
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
async fn missing_source_secret_is_transient() {
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
async fn key_missing_returns_permanent_error() {
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
async fn version_failure_sets_error_and_keeps_scaleway_id() {
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
