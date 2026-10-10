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

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn opt_in_granted_after_error_is_synced_on_next_reconcile() {
    let mut server = mockito::Server::new_async().await;
    mock_find_by_tags(&mut server, r#"{"secrets": []}"#)
        .create_async()
        .await;
    mock_json(&mut server, "POST", SECRETS_PATH, r#"{"id": "sec-late"}"#)
        .create_async()
        .await;
    let version = mock_json(
        &mut server,
        "POST",
        &format!("{SECRETS_PATH}/sec-late/versions"),
        r#"{"revision": 1}"#,
    )
    .expect(1)
    .create_async()
    .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-late-optin");
    let (source, _) = fixture.create_source_secret(&name, "password", false).await;
    fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    let ctx = fixture.ctx(&server.url());

    let before = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    // L'utilisateur corrige le Secret, sans toucher au CR.
    fixture.grant_opt_in(&source, &name).await;
    let after = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let updated = fixture.get_scaleway_secret(&name).await;

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    let err = before.expect_err("expected SecretOptInMissing before the fix");
    // C'est ce délai qui fait revenir le controller sans événement sur le CR.
    assert!(err.recheck_interval().is_some(), "got: {err:?}");
    assert!(after.is_ok(), "Expected Ok, got: {:?}", after);
    version.assert_async().await;
    let status = updated.status.expect("Expected status");
    assert_eq!(status.sync_state, "Synced");
    assert_eq!(status.error_message, None);
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn revoked_secret_is_not_disabled_again_on_recheck() {
    let mut server = mockito::Server::new_async().await;
    let disable = mock_json(
        &mut server,
        "POST",
        &format!("{SECRETS_PATH}/sec-rev-once/versions/3/disable"),
        "{}",
    )
    .expect(1)
    .create_async()
    .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-revoke-once");
    let (source, source_rv) = fixture.create_source_secret(&name, "password", false).await;
    fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    fixture
        .set_scaleway_secret_status(&name, synced_status("sec-rev-once", 3, &source_rv))
        .await;
    let ctx = fixture.ctx(&server.url());

    let first = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let second = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let updated = fixture.get_scaleway_secret(&name).await;

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    assert!(matches!(first, Err(OperatorError::SecretOptInMissing(_))));
    assert!(matches!(second, Err(OperatorError::SecretOptInMissing(_))));
    disable.assert_async().await;
    assert_eq!(
        updated.status.expect("Expected status").sync_state,
        "Revoked"
    );
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn opt_in_restored_after_revocation_pushes_a_new_version() {
    let mut server = mockito::Server::new_async().await;
    mock_json(
        &mut server,
        "POST",
        &format!("{SECRETS_PATH}/sec-restore/versions/3/disable"),
        "{}",
    )
    .create_async()
    .await;
    let version = mock_json(
        &mut server,
        "POST",
        &format!("{SECRETS_PATH}/sec-restore/versions"),
        r#"{"revision": 4}"#,
    )
    .expect(1)
    .create_async()
    .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-restore");
    let (source, source_rv) = fixture.create_source_secret(&name, "password", false).await;
    fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    fixture
        .set_scaleway_secret_status(&name, synced_status("sec-restore", 3, &source_rv))
        .await;
    let ctx = fixture.ctx(&server.url());

    let revoked = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    fixture.grant_opt_in(&source, &name).await;
    let restored = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let updated = fixture.get_scaleway_secret(&name).await;

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    assert!(revoked.is_err());
    assert!(restored.is_ok(), "Expected Ok, got: {:?}", restored);
    version.assert_async().await;
    let status = updated.status.expect("Expected status");
    assert_eq!(status.sync_state, "Synced");
    assert_eq!(status.current_version, Some(4));
    assert_eq!(status.error_message, None);
}
