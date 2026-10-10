//! ScalewaySecret : autorisation du Secret source, révocation et rôle du namespace.

use super::*;

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn opt_in_missing_returns_permanent_error() {
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
async fn opt_in_removed_disables_version_and_marks_revoked() {
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
async fn failed_revocation_is_retried_and_not_marked_revoked() {
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
async fn viewer_role_cannot_sync() {
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
