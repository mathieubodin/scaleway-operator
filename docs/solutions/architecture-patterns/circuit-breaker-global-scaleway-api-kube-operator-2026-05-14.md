---
title: "Circuit breaker global sur un client HTTP dans un opérateur kube-rs"
date: 2026-05-14
category: docs/solutions/architecture-patterns/
module: reconciler
problem_type: architecture_pattern
component: scaleway_client
severity: medium
applies_when:
  - L'opérateur réconcilie un grand nombre de ressources simultanément
  - L'API externe peut être temporairement dégradée (429, 500, timeout)
  - Le backoff exponentiel par ressource n'est pas suffisant pour éviter une rafale de requêtes groupées
  - On veut couper les appels API sans ajouter de dépendance externe
tags:
  - kubernetes-operator
  - rust
  - kube-rs
  - circuit-breaker
  - resilience
  - scaleway-api
  - thundering-herd
---

# Circuit breaker global sur un client HTTP dans un opérateur kube-rs

## Context

Dans un opérateur kube-rs réconciliant N ressources en parallèle, le backoff exponentiel par ressource (géré dans `error_policy`) espace les retries individuels. Mais lors d'une panne Scaleway, N instances accumulent chacune des erreurs — et leurs timers de backoff expirent de façon groupée, générant une rafale de requêtes simultanées contre une API déjà dégradée.

Le circuit breaker résout ce problème en coupant globalement les appels Scaleway dès qu'un seuil d'erreurs consécutives est atteint, indépendamment du nombre de ressources en erreur.

## Guidance

### Architecture retenue

Le circuit breaker vit dans `Context` (partagé entre tous les réconciliateurs via `Arc<Context>`) sous la forme d'un `Mutex<CircuitBreakerState>`. Cette approche suit exactement le pattern `retry_counts: Mutex<HashMap<String, u32>>` déjà présent dans le codebase.

```rust
// src/context.rs
const CIRCUIT_FAILURE_THRESHOLD: u32 = 5;
const CIRCUIT_OPEN_TIMEOUT: Duration = Duration::from_secs(60);

pub enum CircuitBreakerState {
    Closed { failure_count: u32 },
    Open { opened_at: Instant },
    HalfOpen,
}

pub struct Context {
    // ...
    pub circuit_breaker: Mutex<CircuitBreakerState>,
}
```

### Machine à états

```
          5 erreurs transitoires        timeout 60s
Closed ─────────────────────────► Open ──────────────► HalfOpen
  ▲                                                          │
  │                           1 erreur                      │
  │              HalfOpen ──────────────► Open              │
  └───────────────────────── 1 succès ──────────────────────┘
```

- **Closed** : état normal. Compteur d'erreurs consécutives maintenu. Reset sur tout succès.
- **Open** : circuit ouvert. Appels Scaleway bloqués immédiatement (`CircuitBreakerOpen` retourné sans HTTP call).
- **HalfOpen** : timeout écoulé. Un seul appel de sonde autorisé. Succès → Closed, Échec → Open.

La transition Open → HalfOpen est lazy : elle se produit dans `is_circuit_open()` à la lecture, pas via un background task. Simplicité > précision à la milliseconde.

### Placement du check et du wrapper

**Le check** se fait dans `reconcile_instance_inner` **après** le retour anticipé de `handle_deletion` (étape 1). La suppression est intentionnellement exempte du circuit — les ressources doivent pouvoir être supprimées même pendant une panne Scaleway.

```rust
// Après l'étape 1 (deletion early-return), avant les étapes 2-9
if ctx.is_circuit_open() {
    tracing::warn!("Scaleway API circuit breaker is open — skipping reconciliation");
    return Err(OperatorError::CircuitBreakerOpen);
}
```

**Le wrapper** `call_scaleway` encapsule chaque appel HTTP Scaleway pour alimenter l'état du circuit :

```rust
async fn call_scaleway<T, F, Fut>(ctx: &Arc<Context>, f: F) -> Result<T>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let result = f().await;
    match &result {
        Ok(_) => ctx.record_scaleway_success(),
        Err(e) if !is_permanent_error(e) => ctx.record_scaleway_failure(),
        _ => {} // Erreurs permanentes (ConfigError, ProjectAccessDenied...) n'affectent pas le circuit
    }
    result
}
```

Points d'application : `verify_project_access` (via `ctx.scaleway_client`), `create_instance`, `get_instance`, `delete_instance` (via `ns_client`). Les appels Kubernetes (`KubeError`) sont hors scope du circuit.

### Intégration avec error_policy

`CircuitBreakerOpen` est déclaré **transitoire** (`is_permanent_error` retourne `false`) → il bénéficie du backoff exponentiel par instance existant (30s → 300s max). L'`error_policy` logue à `warn` plutôt qu'`error` pour ne pas polluer les alertes :

```rust
if matches!(error, OperatorError::CircuitBreakerOpen) {
    tracing::warn!(attempts, retry_in_secs, "Scaleway API circuit breaker open — backing off");
} else {
    tracing::error!(error = %error, attempts, retry_in_secs, "Transient reconciliation error");
}
```

### Limitations connues

**Compteur global** : le compteur d'erreurs est partagé entre toutes les instances. Instance A (3 échecs) + instance B (2 échecs) = circuit ouvert, même si aucune n'a atteint le seuil individuellement. Ce comportement est intentionnel pour les pannes soutenues, mais ne protège pas contre le flapping (API intermittente). Une fenêtre glissante par endpoint serait l'évolution naturelle.

**HalfOpen thundering herd** : si N instances lisent simultanément `is_circuit_open()` pendant l'état HalfOpen, toutes obtiennent `false` et envoient N appels de sonde en même temps. Acceptable pour des déploiements de taille modérée.

**Pas de persistance cross-restart** : le compteur repart à 0 à chaque redémarrage de l'opérateur.

## Why This Matters

Sans circuit breaker, une panne Scaleway de 60s avec 50 instances en erreur génère environ 50 rafales groupées (une par expiration de backoff par instance). Avec le circuit, une seule sonde est envoyée toutes les 60s, quel que soit le nombre d'instances.

## When to Apply

- Ajouter systématiquement dans tout opérateur kube-rs qui appelle une API externe sous charge multi-ressource.
- Ne pas appliquer sur les appels Kubernetes eux-mêmes (l'API server est dans le cluster — les erreurs kube ont une cause différente des erreurs API tierces).
- Ajuster `CIRCUIT_FAILURE_THRESHOLD` selon la taille du déploiement : 5 est calibré pour des clusters de taille modérée (<100 instances). Augmenter pour les grands clusters.

## Related

- `src/context.rs` — implémentation de `CircuitBreakerState` et des méthodes `is_circuit_open`, `record_scaleway_failure`, `record_scaleway_success`
- `src/reconcilers.rs` — `call_scaleway` wrapper, placement du check, intégration dans `error_policy`
- `src/error.rs` — variant `CircuitBreakerOpen` avec `metric_label()` et `for_status()`
- `docs/solutions/runtime-errors/readyz-503-no-instance-crs-last-reconcile-never-updated-2026-05-11.md` — pattern `Mutex` dans `Context` et heartbeat tokio
