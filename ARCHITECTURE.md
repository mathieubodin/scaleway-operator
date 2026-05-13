# Architecture

Ce document est le point d'entrée pour contribuer à l'opérateur : il décrit comment les modules s'articulent, comment fonctionne la boucle de réconciliation, et comment ajouter le support d'une nouvelle ressource Scaleway.

## Vue d'ensemble des modules

```
main.rs          — Point d'entrée : initialise le Context, lance le Controller kube-rs et le serveur axum
context.rs       — Struct Context partagé entre tous les réconciliateurs (client k8s, client Scaleway, métriques, backoff)
resources.rs     — Définitions des CRDs via #[derive(CustomResource)] : Instance, NamespaceRole
reconcilers.rs   — Logique de réconciliation pour Instance ; error_policy commune
scaleway.rs      — ScalewayClient : wrappeur reqwest sur l'API REST Scaleway
error.rs         — OperatorError enum (10 variants actives + FinalizationError réservé) + sanitization pour le status CRD
metrics.rs       — Compteurs et histogrammes Prometheus + ReconcileMeasurer (RAII timer)
server.rs        — Serveur axum : /healthz, /readyz, /metrics, /log-level
```

## Flux de réconciliation — Instance

`reconcile_instance` est appelé par kube-rs à chaque changement de la ressource ou après un requeue. Il délègue à `reconcile_instance_inner` et réinitialise le compteur de backoff en cas de succès.

Les étapes dans `reconcile_instance_inner` :

```
1. deletion_timestamp ?  → handle_deletion (DELETE Scaleway + retrait finalizer)
2. get NamespaceRole     → ConfigError (permanent) si absent
3. get project-id        → ConfigError (permanent) si annotation absente ou UUID invalide
4. ajouter finalizer     → requeue 5s (le CR va être mis à jour, kube-rs re-triggera)
5. valider zone + type   → InvalidZone / InvalidInstanceType (permanent)
6. vérifier accès projet → ProjectAccessDenied (permanent)
7. créer instance        → requeue 10s si scaleway_id absent (instance en cours de création)
8. synchroniser état     → update status (scaleway_id, state, public_ip)
9. requeue 30s           → synchronisation périodique
```

**Règle de mesure :** `ReconcileMeasurer` est créé via `ReconcileMeasurer::new(...)` en début de chemin et `set_outcome(...)` est appelé avant chaque retour. Le Drop enregistre durée + outcome dans Prometheus. Ne pas oublier le `set_outcome` — le measurer log un warn et enregistre `Error` par défaut.

## Gestion des erreurs

`OperatorError` distingue deux catégories, traitées différemment dans `error_policy` :

| Catégorie | Variants | Comportement |
|-----------|----------|--------------|
| **Permanente** | `ConfigError`, `InvalidZone`, `InvalidInstanceType`, `ProjectAccessDenied` | `Action::await_change()` — pas de retry, attend une modification du CR |
| **Transitoire** | `ScalewayError`, `KubeError`, `NetworkError`, `Unknown` | Backoff exponentiel : 30s → 60s → 120s → 240s → 300s max |

**Règle :** une erreur est permanente si et seulement si un retry immédiat ne peut pas la résoudre — c'est-à-dire si elle nécessite une action de l'utilisateur (corriger le spec, créer une ressource manquante).

**Status CRD :** utiliser `error.for_status()` (pas `error.to_string()`) pour écrire dans `status.error_message`. `for_status()` sanitize les variants qui peuvent contenir des URLs internes ou des adresses d'API.

## Ajouter une nouvelle ressource Scaleway

Voici les étapes pour supporter un nouveau type de ressource (exemple : `LoadBalancer`).

### 1. Définir la CRD dans `resources.rs`

```rust
#[derive(CustomResource, Serialize, Deserialize, Debug, Clone, JsonSchema)]
#[kube(group = "scaleway.mathieubodin.io", version = "v1", kind = "LoadBalancer")]
#[kube(namespaced)]
#[kube(status = "LoadBalancerStatus")]
#[kube(printcolumn = r#"{"name":"State","type":"string","jsonPath":".status.state"}"#)]
pub struct LoadBalancerSpec {
    pub name: String,
    pub zone: String,
    // ...
}

#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema)]
pub struct LoadBalancerStatus {
    #[serde(default)]
    pub scaleway_id: Option<String>,
    #[serde(default)]
    pub state: String,
    // ...
}

impl Default for LoadBalancerStatus { ... }
```

### 2. Ajouter les appels API dans `scaleway.rs`

Implémenter les méthodes sur `ScalewayClient` : `create_load_balancer`, `get_load_balancer`, `delete_load_balancer`. Suivre le même pattern que les méthodes Instance : POST/GET/DELETE sur l'URL Scaleway, parse de la réponse JSON, mapping des codes d'erreur HTTP vers `OperatorError`.

### 3. Écrire le réconciliateur dans `reconcilers.rs`

```rust
pub async fn reconcile_load_balancer(
    lb: Arc<LoadBalancer>,
    ctx: Arc<Context>,
) -> std::result::Result<Action, OperatorError> {
    let key = format!("{}/{}", lb.namespace().unwrap_or_default(), lb.name_any());
    let result = reconcile_load_balancer_inner(lb, ctx.clone()).await;
    if result.is_ok() {
        ctx.reset_retry_count(&key);
    }
    result
}
```

Suivre les mêmes étapes que `reconcile_instance_inner` : deletion en priorité, NamespaceRole, project-id, finalizer, validation, création, synchronisation, requeue.

Utiliser `ReconcileMeasurer` et appeler `set_outcome` avant chaque return.

### 4. Brancher dans `main.rs`

```rust
let lb_api = Api::<LoadBalancer>::all(client.clone());
Controller::new(lb_api, Default::default())
    .run(reconcile_load_balancer, error_policy, Arc::clone(&context))
    .for_each(|res| async move { ... })
    .await;
```

`error_policy` est générique (elle ne connaît pas le type de ressource) — elle peut être réutilisée telle quelle si les mêmes catégories d'erreurs s'appliquent.

### 5. Mettre à jour `examples/crd_gen.rs`

```rust
use scaleway_operator::resources::{Instance, LoadBalancer, NamespaceRole};
// ...
write_crd("k8s/crd-loadbalancer.yaml", &LoadBalancer::crd(), None);
```

Régénérer les fichiers YAML : `make generate-crds`.

## Tests

### Tests unitaires

Chaque module a ses tests dans un bloc `#[cfg(test)]` en bas du fichier. Lancer avec :

```bash
make test
```

Pour les tests de `reconcilers.rs`, utiliser `make_test_context()` (défini dans le module de test) qui construit un `Context` avec un client kube factice et un `OperatorMetrics` frais.

### Tests d'intégration

Dans `tests/integration.rs` — nécessitent un cluster Kubernetes accessible et les CRDs déployées. Ils sont marqués `#[ignore]` et s'exécutent localement uniquement.

Lancer en une commande (déploie les CRDs + fixtures + lance les tests) :

```bash
make run-integration-test-locally
```

Ou étape par étape :

```bash
kubectl proxy --port=8001 &
make deploy-crds
make deploy-test-fixtures
make test-integration
```

### Régénérer les fichiers CRD YAML

Après toute modification de `resources.rs` :

```bash
make generate-crds
```

Génère `k8s/crd-instance.yaml` et `k8s/crd-namespacerole.yaml`.

