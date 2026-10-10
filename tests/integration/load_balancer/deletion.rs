//! LoadBalancer : finalizer et suppression.

use super::*;
use std::sync::Arc;

#[tokio::test]
#[ignore = "requires make test-integration-kind"]
async fn finalizer_added_on_first_reconcile() {
    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let server = mockito::Server::new_async().await;

    // Mock NamespaceRole lookup (handled by kube API, not mocked here)
    // The reconciler should add the finalizer on the first reconcile cycle.

    let name = unique_name("lb-test");
    let lb_api: Api<LoadBalancer> = Api::namespaced(fixture.client.clone(), NS_EDITOR);

    let lb = lb_api
        .create(
            &PostParams::default(),
            &build_load_balancer(NS_EDITOR, &name),
        )
        .await
        .expect("Failed to create LoadBalancer CR");

    let ctx = fixture.ctx(&server.url());
    let result = scaleway_operator::reconcilers::reconcile_load_balancer(Arc::new(lb), ctx).await;

    let updated = lb_api
        .get(&name)
        .await
        .expect("Failed to re-fetch LoadBalancer");
    let _ = lb_api.delete(&name, &DeleteParams::default()).await;
    drop(server);

    // AddFinalizer returns Ok(requeue 5s)
    assert!(
        result.is_ok(),
        "Expected Ok on first reconcile, got {:?}",
        result
    );
    assert!(
        updated
            .metadata
            .finalizers
            .unwrap_or_default()
            .contains(&"scaleway.mathieubodin.io/loadbalancer-finalizer".to_string()),
        "Finalizer should be present after first reconcile"
    );
}
