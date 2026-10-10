//! LoadBalancer : création, adoption et synchronisation.

use super::*;

#[tokio::test]
#[ignore = "requires live Scaleway credentials with LB permissions — see #119"]
async fn create_sync_delete() {
    // Pré-requis (à câbler par CI maintainer ou ops, voir issue #119) :
    // - Variable d'env SCALEWAY_LB_LIVE_TEST=1 pour signaler le mode live
    // - Secret K8s `scaleway-ns-creds-scw-test-editor` (déjà dans test-fixtures.yaml)
    //   contient une clé IAM Scaleway avec LoadBalancerFullAccess sur un projet sandbox
    // - Annotation `scaleway.mathieubodin.io/project-id` du namespace `scw-test-editor`
    //   pointe vers ce projet sandbox
    //
    // Flow :
    // 1. Créer un LoadBalancer CR avec un nom unique (pour éviter collisions)
    // 2. Réconcilier (boucle wait jusqu'à 60s, requeue 5s puis 30s)
    //    → status.scaleway_id doit être populated
    //    → status.state doit converger vers "ready" ou "running" selon le payload Scaleway
    //    → status.public_ip doit être présent
    // 3. Supprimer le CR
    // 4. Re-réconcilier (boucle wait jusqu'à 60s)
    //    → la suppression Scaleway doit être appelée (DELETE /lb/v1/zones/.../lbs/...)
    //    → le finalizer doit être retiré
    //    → le CR doit disparaître de l'API K8s
    //
    // ⚠️ Coût : l'exécution réelle consomme un LB Scaleway pendant ~1 minute.
    //    À exécuter dans un projet sandbox uniquement.

    if std::env::var("SCALEWAY_LB_LIVE_TEST").is_err() {
        eprintln!(
            "load_balancer::sync::create_sync_delete: SCALEWAY_LB_LIVE_TEST non défini, skip"
        );
        return;
    }

    let fixture = TestFixture::for_namespace(NS_EDITOR).await;
    let name = unique_name("scw-lb-live");
    let lb_api: Api<LoadBalancer> = Api::namespaced(fixture.client.clone(), NS_EDITOR);

    // Cleanup en garantie (cas où un test précédent a laissé un orphelin)
    let _ = lb_api.delete(&name, &DeleteParams::default()).await;

    // -- TODO(#119) -- Phase 1 : Apply CR + assertion status.scaleway_id Some
    // -- TODO(#119) -- Phase 2 : Wait state convergence + public_ip
    // -- TODO(#119) -- Phase 3 : Delete CR + assertion finalizer removed
    //
    // Implementation requires the `wait_for_predicate(timeout, interval, f)` helper
    // (not yet extracted). When this test is enabled, factor that helper out of
    // existing Instance integration tests for reuse.

    unimplemented!("test scaffold — see #119 for implementation tracking");
}
