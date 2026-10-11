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

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn past_error_is_cleared_when_nothing_is_left_to_push() {
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
    let name = unique_name("scw-secret-stuck");
    let (source, source_rv) = fixture.create_source_secret(&name, "password", true).await;
    let created = fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    // Status laissé par une erreur passagère de lecture du Secret source, après une synchro.
    let mut stuck = synced_status("sec-stuck", 2, &source_rv);
    stuck.observed_generation = created.metadata.generation;
    stuck.sync_state = "Error".to_string();
    stuck.error_message = Some("Source Secret unavailable (see operator logs)".to_string());
    fixture.set_scaleway_secret_status(&name, stuck).await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let updated = fixture.get_scaleway_secret(&name).await;

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    any_get.assert_async().await;
    any_post.assert_async().await;
    let status = updated.status.expect("Expected status");
    assert_eq!(status.sync_state, "Synced");
    assert_eq!(status.error_message, None);
    assert_eq!(status.current_version, Some(2));
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn lookup_failure_sets_error() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", SECRETS_PATH)
        .match_query(mockito::Matcher::Any)
        .with_status(500)
        .with_body("boom")
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-findko");
    let (source, _) = fixture.create_source_secret(&name, "password", true).await;
    fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let updated = fixture.get_scaleway_secret(&name).await;

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    let err = result.expect_err("expected the lookup error");
    assert!(
        matches!(err, OperatorError::ScalewayError { .. }),
        "got: {err:?}"
    );
    let status = updated.status.expect("Expected status");
    assert_eq!(status.sync_state, "Error");
    assert!(status.error_message.is_some());
    assert_eq!(status.scaleway_id, None);
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn creation_failure_sets_error() {
    let mut server = mockito::Server::new_async().await;
    mock_find_by_tags(&mut server, r#"{"secrets": []}"#)
        .create_async()
        .await;
    server
        .mock("POST", SECRETS_PATH)
        .with_status(500)
        .with_body("boom")
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-createko");
    let (source, _) = fixture.create_source_secret(&name, "password", true).await;
    fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let updated = fixture.get_scaleway_secret(&name).await;

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    let err = result.expect_err("expected the creation error");
    assert!(
        matches!(err, OperatorError::ScalewayError { .. }),
        "got: {err:?}"
    );
    let status = updated.status.expect("Expected status");
    assert_eq!(status.sync_state, "Error");
    assert!(status.error_message.is_some());
    assert_eq!(status.scaleway_id, None);
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn rotation_failure_sets_error_and_keeps_the_tracked_version() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock(
            "POST",
            format!("{SECRETS_PATH}/sec-rotko/versions").as_str(),
        )
        .with_status(500)
        .with_body("boom")
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-rotko");
    let (source, _) = fixture.create_source_secret(&name, "password", true).await;
    fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    // Le Secret source a tourné depuis la dernière synchro.
    fixture
        .set_scaleway_secret_status(&name, synced_status("sec-rotko", 4, "stale-rv"))
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
    assert!(status.error_message.is_some());
    // Le suivi reste celui de la dernière synchro réussie : le prochain tour retente l'envoi.
    assert_eq!(status.scaleway_id.as_deref(), Some("sec-rotko"));
    assert_eq!(status.current_version, Some(4));
    assert_eq!(
        status.last_synced_resource_version.as_deref(),
        Some("stale-rv")
    );
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
    let name = unique_name("scw-secret-tags");
    let (source, _) = fixture.create_source_secret(&name, "password", true).await;
    fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    // Tags d'un autre CR : son secret Scaleway serait adopté par celui-ci.
    fixture
        .patch_scaleway_secret_spec(
            &name,
            serde_json::json!({ "tags": ["team=payments", "scaleway-operator-cr-name=other-cr"] }),
        )
        .await
        .expect("tags are accepted by the API server");
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let updated = fixture.get_scaleway_secret(&name).await;

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    let err = result.expect_err("expected a reserved tag error");
    assert!(matches!(err, OperatorError::ConfigError(_)), "got: {err:?}");
    assert!(err.is_permanent_error());
    any_get.assert_async().await;
    any_post.assert_async().await;
    let status = updated.status.expect("Expected status");
    assert_eq!(status.sync_state, "Error");
    assert!(
        status
            .error_message
            .as_deref()
            .is_some_and(|m| m.contains("scaleway-operator-cr-name=other-cr")),
        "got: {:?}",
        status.error_message
    );
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn region_change_is_refused_by_the_api_server() {
    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-region");
    let (source, _) = fixture.create_source_secret(&name, "password", true).await;
    fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;

    let result = fixture
        .patch_scaleway_secret_spec(&name, serde_json::json!({ "region": "nl-ams" }))
        .await;
    let updated = fixture.get_scaleway_secret(&name).await;

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    let err = result.expect_err("expected the region change to be refused");
    assert!(
        err.to_string().contains("spec.region is immutable"),
        "got: {err}"
    );
    assert_eq!(updated.spec.region, "fr-par");
}
