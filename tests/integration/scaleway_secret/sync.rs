//! ScalewaySecret : création, adoption et rotation.

use super::*;

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn create_writes_status_from_source_resource_version() {
    let mut server = mockito::Server::new_async().await;
    let find = mock_find_by_tags(&mut server, r#"{"secrets": []}"#)
        .expect(1)
        .create_async()
        .await;
    let create = mock_json(
        &mut server,
        "POST",
        SECRETS_PATH,
        r#"{"id": "sec-created"}"#,
    )
    .expect(1)
    .create_async()
    .await;
    let version = mock_json(
        &mut server,
        "POST",
        &format!("{SECRETS_PATH}/sec-created/versions"),
        r#"{"revision": 1}"#,
    )
    .expect(1)
    .create_async()
    .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-create");
    let (source, source_rv) = fixture.create_source_secret(&name, "password", true).await;
    fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let updated = fixture.get_scaleway_secret(&name).await;

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    find.assert_async().await;
    create.assert_async().await;
    version.assert_async().await;
    let status = updated.status.expect("Expected status");
    assert_eq!(status.scaleway_id.as_deref(), Some("sec-created"));
    assert_eq!(status.current_version, Some(1));
    assert_eq!(status.last_synced_resource_version, Some(source_rv));
    assert_eq!(status.observed_generation, updated.metadata.generation);
    assert_eq!(status.sync_state, "Synced");
    assert_eq!(status.error_message, None);
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn adoption_does_not_call_create() {
    let mut server = mockito::Server::new_async().await;
    mock_find_by_tags(&mut server, r#"{"secrets": [{"id": "sec-adopted"}]}"#)
        .create_async()
        .await;
    let create = mock_json(&mut server, "POST", SECRETS_PATH, r#"{"id": "sec-other"}"#)
        .expect(0)
        .create_async()
        .await;
    let version = mock_json(
        &mut server,
        "POST",
        &format!("{SECRETS_PATH}/sec-adopted/versions"),
        r#"{"revision": 4}"#,
    )
    .expect(1)
    .create_async()
    .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-adopt");
    let (source, _) = fixture.create_source_secret(&name, "password", true).await;
    fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let updated = fixture.get_scaleway_secret(&name).await;

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    create.assert_async().await;
    version.assert_async().await;
    let status = updated.status.expect("Expected status");
    assert_eq!(status.scaleway_id.as_deref(), Some("sec-adopted"));
    assert_eq!(status.current_version, Some(4));
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn rotation_survives_failed_disable_without_repush() {
    let mut server = mockito::Server::new_async().await;
    let version = mock_json(
        &mut server,
        "POST",
        &format!("{SECRETS_PATH}/sec-rot/versions"),
        r#"{"revision": 2}"#,
    )
    .expect(1)
    .create_async()
    .await;
    let disable = server
        .mock(
            "POST",
            format!("{SECRETS_PATH}/sec-rot/versions/1/disable").as_str(),
        )
        .with_status(500)
        .with_body("boom")
        .expect(1)
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-rot");
    let (source, source_rv) = fixture.create_source_secret(&name, "password", true).await;
    fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    fixture
        .set_scaleway_secret_status(&name, synced_status("sec-rot", 1, "stale-rv"))
        .await;
    let ctx = fixture.ctx(&server.url());

    let first = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let second = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let updated = fixture.get_scaleway_secret(&name).await;

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    assert!(first.is_ok(), "Expected Ok, got: {:?}", first);
    assert!(second.is_ok(), "Expected Ok, got: {:?}", second);
    // Une seule version créée malgré deux réconciliations et un disable en échec.
    version.assert_async().await;
    disable.assert_async().await;
    let status = updated.status.expect("Expected status");
    assert_eq!(status.current_version, Some(2));
    assert_eq!(status.last_synced_resource_version, Some(source_rv));
    assert_eq!(status.sync_state, "Synced");
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn spec_key_change_pushes_the_new_key_without_source_change() {
    let mut server = mockito::Server::new_async().await;
    // base64("value-of-token")
    let version = mock_json(
        &mut server,
        "POST",
        &format!("{SECRETS_PATH}/sec-key/versions"),
        r#"{"revision": 2}"#,
    )
    .match_body(mockito::Matcher::PartialJsonString(
        r#"{"data": "dmFsdWUtb2YtdG9rZW4="}"#.to_string(),
    ))
    .expect(1)
    .create_async()
    .await;
    mock_json(
        &mut server,
        "POST",
        &format!("{SECRETS_PATH}/sec-key/versions/1/disable"),
        "{}",
    )
    .create_async()
    .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-key");
    let (source, source_rv) = fixture
        .create_source_secret_with_keys(&name, &["password", "token"], true)
        .await;
    let created = fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    fixture
        .set_scaleway_secret_status(
            &name,
            ScalewaySecretStatus {
                observed_generation: created.metadata.generation,
                ..synced_status("sec-key", 1, &source_rv)
            },
        )
        .await;
    let api: Api<ScalewaySecret> = Api::namespaced(fixture.client.clone(), NS_EDITOR);
    let change_key = serde_json::json!({
        "spec": { "source": { "kubernetes_secret": { "key": "token" } } }
    });
    api.patch(&name, &PatchParams::default(), &Patch::Merge(change_key))
        .await
        .expect("change the key read by the CR");
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let updated = fixture.get_scaleway_secret(&name).await;

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    version.assert_async().await;
    let status = updated.status.expect("Expected status");
    assert_eq!(status.current_version, Some(2));
    assert_eq!(status.observed_generation, updated.metadata.generation);
    assert_ne!(status.observed_generation, created.metadata.generation);
    // Le Secret source n'a pas changé : seul le spec du CR a déclenché l'envoi.
    assert_eq!(status.last_synced_resource_version, Some(source_rv));
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn status_without_observed_generation_records_it_without_repush() {
    let mut server = mockito::Server::new_async().await;
    let any_post = server
        .mock("POST", mockito::Matcher::Any)
        .expect(0)
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-legacy");
    let (source, source_rv) = fixture.create_source_secret(&name, "password", true).await;
    fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    // Status tel qu'écrit par une version de l'opérateur qui ne suivait pas la generation.
    fixture
        .set_scaleway_secret_status(&name, synced_status("sec-legacy", 1, &source_rv))
        .await;
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let updated = fixture.get_scaleway_secret(&name).await;

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    any_post.assert_async().await;
    let status = updated.status.expect("Expected status");
    assert_eq!(status.observed_generation, updated.metadata.generation);
    assert_eq!(status.current_version, Some(1));
}
