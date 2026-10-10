//! ScalewaySecret : finalizer et suppression.

use super::*;

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn finalizer_added_on_first_reconcile() {
    let server = mockito::Server::new_async().await;
    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-finalizer");
    let (source, _) = fixture.create_source_secret(&name, "password", true).await;
    fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    let api: Api<ScalewaySecret> = Api::namespaced(fixture.client.clone(), NS_EDITOR);
    let remove_finalizer = serde_json::json!({ "metadata": { "finalizers": null } });
    api.patch(
        &name,
        &PatchParams::default(),
        &Patch::Merge(remove_finalizer),
    )
    .await
    .expect("remove finalizer");
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let updated = fixture.get_scaleway_secret(&name).await;

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    assert_eq!(
        updated.metadata.finalizers,
        Some(vec![SECRET_FINALIZER.to_string()])
    );
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn deletion_calls_delete_api_and_removes_finalizer() {
    let mut server = mockito::Server::new_async().await;
    let delete = server
        .mock("DELETE", format!("{SECRETS_PATH}/sec-del").as_str())
        .with_status(204)
        .expect(1)
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-delete");
    let (source, source_rv) = fixture.create_source_secret(&name, "password", true).await;
    fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    fixture
        .set_scaleway_secret_status(&name, synced_status("sec-del", 1, &source_rv))
        .await;
    let api: Api<ScalewaySecret> = Api::namespaced(fixture.client.clone(), NS_EDITOR);
    // Le finalizer retient l'objet : il reste lisible avec un deletionTimestamp.
    api.delete(&name, &DeleteParams::default())
        .await
        .expect("delete ScalewaySecret");
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let after = api.get_opt(&name).await.expect("get_opt ScalewaySecret");

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    delete.assert_async().await;
    assert!(
        after.is_none(),
        "the CR should be gone once its finalizer is removed"
    );
}

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn failed_deletion_keeps_finalizer() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("DELETE", format!("{SECRETS_PATH}/sec-del-ko").as_str())
        .with_status(500)
        .with_body("boom")
        .create_async()
        .await;

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-secret-delete-ko");
    let (source, source_rv) = fixture.create_source_secret(&name, "password", true).await;
    fixture
        .create_scaleway_secret(&name, &source, "password")
        .await;
    fixture
        .set_scaleway_secret_status(&name, synced_status("sec-del-ko", 1, &source_rv))
        .await;
    let api: Api<ScalewaySecret> = Api::namespaced(fixture.client.clone(), NS_EDITOR);
    api.delete(&name, &DeleteParams::default())
        .await
        .expect("delete ScalewaySecret");
    let ctx = fixture.ctx(&server.url());

    let result = fixture.reconcile_scaleway_secret(&name, &ctx).await;
    let after = api.get_opt(&name).await.expect("get_opt ScalewaySecret");

    fixture.cleanup_scaleway_secret(&name).await;
    fixture.cleanup_source_secret(&source).await;

    assert!(result.is_err(), "Expected Err, got: {:?}", result);
    let finalizers = after
        .expect("the CR must survive a failed Scaleway deletion")
        .metadata
        .finalizers;
    assert_eq!(finalizers, Some(vec![SECRET_FINALIZER.to_string()]));
}
