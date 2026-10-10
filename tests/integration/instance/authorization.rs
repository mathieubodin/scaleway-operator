//! Instance : rôle du namespace.

use super::*;
use std::sync::Arc;

#[tokio::test]
#[ignore = "requires: make test-integration-kind"]
async fn viewer_role_cannot_create_instance() {
    // Finalizer pré-présent obligatoire pour atteindre l'étape 8 (vérification read-only)
    // Pas de mock Scaleway : le réconciliateur retourne Err avant verify_project_access
    let server = mockito::Server::new_async().await;
    let fixture = TestFixture::for_namespace(NS_VIEWER).await;
    let ctx = fixture.ctx(&server.url());

    let result = scaleway_operator::reconcilers::reconcile_instance(
        Arc::new(build_instance_with_finalizer(NS_VIEWER, "any")),
        ctx,
    )
    .await;

    let err = result.unwrap_err();
    assert!(err.to_string().contains("read-only"), "Got: {}", err);
}
