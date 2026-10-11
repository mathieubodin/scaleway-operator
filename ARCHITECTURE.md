# Architecture

Ce document est le point d'entrée pour contribuer à l'opérateur : il décrit comment les modules s'articulent, comment fonctionne la boucle de réconciliation, et comment ajouter le support d'une nouvelle ressource Scaleway.

## Vue d'ensemble des modules

```text
main.rs          — Point d'entrée : initialise le Context, lance le Controller kube-rs et le serveur axum
context.rs       — Struct Context partagé entre tous les réconciliateurs (client k8s, client Scaleway, métriques, backoff)
resources.rs     — Définitions des CRDs via #[derive(CustomResource)] : Instance, LoadBalancer, ScalewaySecret, RDB, NamespaceRole
reconcilers.rs   — Logique de réconciliation pour Instance, LoadBalancer, ScalewaySecret et RDB ; error_policy commune
scaleway.rs      — ScalewayClient : wrappeur reqwest sur l'API REST Scaleway
error.rs         — OperatorError enum (10 variants actives + FinalizationError réservé) + sanitization pour le status CRD
metrics.rs       — Compteurs et histogrammes Prometheus + ReconcileMeasurer (RAII timer)
server.rs        — Serveur axum : /healthz, /readyz, /metrics, /log-level
```

## Flux de réconciliation — Instance

`reconcile_instance` est appelé par kube-rs à chaque changement de la ressource ou après un requeue. Il délègue à `reconcile_instance_inner` et réinitialise le compteur de backoff en cas de succès.

Les étapes dans `reconcile_instance_inner` :

```text
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

**Règle de mesure :** `ReconcileMeasurer` est créé via `ReconcileMeasurer::new(...)` en début de chemin et
`set_outcome(...)` est appelé avant chaque retour. Le Drop enregistre durée + outcome dans Prometheus. Ne pas
oublier le `set_outcome` — le measurer log un warn et enregistre `Error` par défaut.

## Flux de réconciliation — LoadBalancer

Même enchaînement que pour `Instance`, adapté aux LB Scaleway :

```text
1. deletion_timestamp ?  → DELETE LB Scaleway + retrait du finalizer scaleway.mathieubodin.io/loadbalancer-finalizer
2. get NamespaceRole     → ConfigError (permanent) si absent
3. get project-id        → ConfigError (permanent) si annotation absente ou UUID invalide
4. ajouter finalizer     → requeue
5. valider zone + type   → InvalidZone / InvalidLbType (permanent), listes statiques dans scaleway.rs
6. vérifier accès projet → ProjectAccessDenied (permanent)
7. créer le LB           → si status.scaleway_id est absent
8. synchroniser état     → le LB transite par `pending` avant `ready`
9. requeue périodique    → suivi de la convergence et des dérives
```

## Flux de réconciliation — ScalewaySecret

`ScalewaySecret` pousse la valeur d'une clé d'un Secret Kubernetes vers Scaleway Secret Manager.
Il n'a pas de zone mais une région, et sa source vit dans le cluster.

```text
1. valider spec.region   → ConfigError (permanent), suppression comprise : la région part dans le chemin des URLs
   valider spec.tags     → hors suppression ; ConfigError (permanent) si un tag est réservé à l'opérateur
2. get NamespaceRole     → hors suppression ; ConfigError (permanent) si absent ou en lecture seule
3. lire le Secret source → un seul `get`, qui vérifie l'opt-in et fournit la valeur
                           SecretOptInMissing / SecretKeyNotFound (permanent), SecretNotFound (transitoire)
4. opt-in retiré ?       → désactive la version Scaleway courante, puis status `Revoked`
5. deletion_timestamp ?  → DELETE du secret Scaleway + retrait du finalizer scaleway.mathieubodin.io/secret-finalizer
6. ajouter finalizer     → requeue 5s
7. créer ou adopter      → recherche par tags opérateur (namespace et nom du CR), création sinon, puis première version
8. rotation              → nouvelle version si le resourceVersion du Secret source a changé
                           ou si le spec du CR a changé (status.observed_generation) ;
                           Scaleway désactive la version précédente dans le même appel
9. requeue 30s           → détection de la prochaine rotation
```

Le `status` ne contient jamais de dérivé de la valeur : la rotation est détectée par le `metadata.resourceVersion`
du Secret source. Le compromis est documenté dans
[`docs/solutions/architecture-patterns/scaleway-secret-resource-version-rotation-detection-2026-06-13.md`](docs/solutions/architecture-patterns/scaleway-secret-resource-version-rotation-detection-2026-06-13.md).

Une seule version reste active. L'envoi d'une version et la mise à jour du status ne sont pas atomiques : si la seconde
échoue, le tour suivant renvoie une version. Chaque création passe donc `disable_previous` à Scaleway, qui désactive la
version précédente, connue ou non du status.

Avant de créer un secret ou de pousser une version, le reconciler relit le CR sur l'API server. Si son `resourceVersion`
diffère de celui reçu du cache du controller, il ne pousse rien et repasse 5 secondes plus tard : un status écrit par
le tour précédent mais pas encore vu par le cache ferait pousser une seconde version identique.

`spec.region` est immuable, par une règle CEL du CRD : l'opérateur ne déplace pas un secret d'une région à l'autre.

### Révocation et suppression

Seul le retrait du label ou de l'annotation d'opt-in désactive la version Scaleway. Un Secret source supprimé, ou que
l'opérateur n'a plus le droit de lire, passe le CR en `Error` et laisse la version active : une rotation par suppression
puis recréation du Secret, ou un redéploiement du RBAC, ne doit pas couper les consommateurs du secret.

À la suppression du CR, le secret Scaleway est supprimé avant le retrait du finalizer. Si les credentials du namespace
(`scaleway-ns-creds-<namespace>`) sont absents, l'appel Scaleway est impossible : le finalizer est retiré et le secret
reste dans Scaleway, comme pour `Instance` et `LoadBalancer`. Son identifiant figure dans le log `warn` de l'opérateur.

### Opt-in du Secret source

L'opérateur ne lit un Secret Kubernetes que s'il porte les deux marqueurs suivants :

- le label `scaleway.mathieubodin.io/allow-operator-read: "true"`, chaîne exacte ;
- l'annotation `scaleway.mathieubodin.io/allowed-cr: "<namespace>/<nom du CR>"`, strictement égale au CR qui le référence.

```yaml
apiVersion: v1
kind: Secret
metadata:
  name: db-password
  namespace: team-a
  labels:
    scaleway.mathieubodin.io/allow-operator-read: "true"
  annotations:
    scaleway.mathieubodin.io/allowed-cr: "team-a/db-sync"
```

L'annotation empêche un autre CR du namespace de réutiliser un Secret déjà labellisé.
Le droit de lecture vient d'un Role `get secrets` par namespace, créé par `namespace-bootstrap.yaml` :
le ClusterRole de l'opérateur ne donne aucun accès aux Secrets.

## Flux de réconciliation — RDB

`RDB` provisionne une instance de base de données managée. Le kind est `RDB`, le type Rust `Rdb`.

```text
1. valider spec.region   → ConfigError (permanent), suppression comprise
   valider spec.tags     → hors suppression ; ConfigError (permanent) si un tag est réservé à l'opérateur
2. get NamespaceRole     → hors suppression ; project_id lu sur l'annotation du namespace
3. deletion_timestamp ?  → deletion_policy Delete : DELETE de l'instance, puis retrait du finalizer
                           scaleway.mathieubodin.io/rdb-finalizer ; Retain : retrait du finalizer seul
4. ajouter finalizer     → requeue 5s
5. instance connue ?     → GET, mise à jour du status, signalement des dérives, requeue 30s
6. rôle en lecture seule → ConfigError (permanent) : rien à créer
7. adopter ou créer      → recherche par tags opérateur ; sinon lecture du mot de passe et création
```

`sync_state` prend les valeurs suivantes :

| Valeur | Sens |
| --- | --- |
| `Pending` | Rien n'est créé : le `ScalewaySecret` du mot de passe n'existe pas ou n'est pas `Synced` |
| `Syncing` | Instance créée ou adoptée, pas encore `ready` côté Scaleway |
| `Synced` | Instance `ready` |
| `ConfigDrift` | Le moteur de l'instance n'est plus celui du spec |
| `Error` | Dernier appel en échec, détail dans `error_message` |
| `Deleting` | Scaleway a refusé la suppression (4xx), nouvelle tentative toutes les 30 secondes |

**Mot de passe.** Il est lu dans Secret Manager à partir du `ScalewaySecret` référencé, uniquement quand il faut créer
l'instance : une instance adoptée a déjà le sien. Tant que le `ScalewaySecret` n'est pas `Synced`, le reconcile
renvoie `ScalewaySecretNotReady`, revérifié toutes les 30 secondes car le controller RDB ne surveille pas les
`ScalewaySecret`.

**Idempotence.** L'instance porte les mêmes tags opérateur qu'un secret (namespace et nom du CR). Si la création
réussit mais que l'écriture du status échoue, le tour suivant retrouve l'instance par ses tags au lieu d'en créer
une seconde. Plusieurs instances portant ces tags sont une erreur permanente : en adopter une au hasard pourrait
être faux. Avant d'adopter ou de créer, le reconciler vérifie comme pour `ScalewaySecret` que son CR n'est pas
issu d'un cache en retard.

**Dérives.** L'opérateur ne modifie jamais une instance existante. Un écart sur `node_type`, `is_ha_cluster` ou
`volume_size_gb` est écrit dans `status.drift_message` et émis en Event Warning `SpecDrift`, une seule fois par
apparition ou changement ; `sync_state` reste `Synced`. Un écart sur `engine` passe en `ConfigDrift` :
`spec.engine` reste modifiable pour suivre une montée de version faite côté Scaleway, alors que `spec.region` et
`spec.admin_user` sont immuables par règle CEL.

**Instance disparue.** Un 404 sur une instance connue efface son identifiant du status ; le tour suivant en crée
une nouvelle, vide.

## Prérequis par namespace

Chaque namespace qui héberge des ressources de l'opérateur doit avoir :

- l'annotation `scaleway.mathieubodin.io/project-id` sur le namespace ;
- une ressource `NamespaceRole` cluster-wide dont le nom est exactement celui du namespace ;
- un Secret `scaleway-ns-creds-{namespace}` dans le namespace `scaleway-system`, qui porte les credentials
  Scaleway utilisés pour ce namespace.

## Gestion des erreurs

`OperatorError` distingue deux catégories, traitées différemment dans `error_policy` :

|Catégorie|Variants|Comportement|
|---|---|---|
|**Permanente**|`ConfigError`, `InvalidZone`, `InvalidInstanceType`, `InvalidLbType`, `ProjectAccessDenied`, `SecretSourceNotConfigured`, `SecretOptInMissing`, `SecretKeyNotFound`, `SecretAccessDenied`|`Action::await_change()` — pas de retry, attend une modification du CR|
|**Transitoire**|`ScalewayError`, `KubeError`, `NetworkError`, `SecretNotFound`, `RdbNotFound`, `ScalewaySecretNotReady`, `Unknown`|Backoff exponentiel : 30s → 60s → 120s → 240s → 300s max|

**Revérification :** `SecretOptInMissing`, `SecretKeyNotFound` et `SecretAccessDenied` sont permanentes mais se corrigent
sur le Secret source ou son RBAC, que le controller ne surveille pas. `error_policy` les revérifie toutes les 5 minutes
(`OperatorError::recheck_interval`) au lieu d'attendre une modification du CR.
`ScalewaySecretNotReady` est transitoire mais revérifiée toutes les 30 secondes par le même mécanisme : le backoff
retarderait la création d'une instance RDB jusqu'à cinq minutes après que son `ScalewaySecret` est prêt.

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

Dans `tests/integration/`. Ils exécutent les reconcilers contre un vrai cluster Kubernetes, avec une API Scaleway
simulée par mockito. Ils sont marqués `#[ignore]` pour ne tourner que dans ce contexte.

```bash
make test-integration-kind
```

La cible crée un cluster kind éphémère, y déploie les CRDs et `k8s/test-fixtures.yaml`, lance les tests, puis détruit le cluster.

Le dossier suit le même schéma pour chaque ressource :

```text
tests/integration/
├── main.rs                  point d'entrée, liste des modules
├── support/                 connexion au cluster, namespaces de test, fausses réponses Scaleway
└── <ressource>/             instance, load_balancer, scaleway_secret
    ├── mod.rs               construction et nettoyage des objets de la ressource
    ├── sync.rs              création, adoption, synchronisation
    ├── authorization.rs     rôle du namespace, autorisations
    ├── errors.rs            prérequis manquants, erreurs de l'API Scaleway
    └── deletion.rs          finalizer et suppression
```

Une ressource n'a que les fichiers dont elle a besoin. Le nom complet d'un test se lit comme un chemin,
par exemple `scaleway_secret::authorization::viewer_role_cannot_sync`.

Chaque test crée ses propres ressources et les supprime en fin de test. Un test incomplet doit être ajouté
à la liste `--skip` de `scripts/test-integration-kind.sh` : `#[ignore]` ne l'exclut pas.

### Régénérer les fichiers CRD YAML

Après toute modification de `resources.rs` :

```bash
make generate-crds
```

Génère `k8s/crd-instance.yaml` et `k8s/crd-namespacerole.yaml`.
