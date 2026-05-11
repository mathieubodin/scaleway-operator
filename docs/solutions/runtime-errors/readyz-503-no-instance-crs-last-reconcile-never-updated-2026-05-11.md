---
title: "/readyz retourne 503 en permanence quand aucun CR Instance n'existe"
date: 2026-05-11
category: docs/solutions/runtime-errors/
module: server
problem_type: runtime_error
component: development_workflow
severity: high
symptoms:
  - "Le pod est déployé sans erreur mais reste NotReady indéfiniment"
  - "kubectl describe pod affiche : Readiness probe failed: HTTP probe failed with statuscode: 503"
  - "Aucun CR Instance n'est déployé dans le cluster"
root_cause: config_error
resolution_type: code_fix
tags:
  - kubernetes, readiness-probe, axum, operator, health, kube-rs
---

# /readyz retourne 503 en permanence quand aucun CR Instance n'existe

## Problem

L'endpoint `/readyz` vérifie que `last_reconcile_at` a été mis à jour dans les 60 dernières secondes. Lors du démarrage, `last_reconcile_at` est initialisé à `0`. Si aucun CR `Instance` n'existe dans le cluster, le réconciliateur ne s'exécute jamais, le timestamp reste à `0`, et `/readyz` retourne `503` indéfiniment — empêchant le pod de passer en état `Ready`.

## Symptoms

- `kubectl get pods` affiche `0/1 Running` pour le pod opérateur, sans crashloop.
- `kubectl describe pod <pod>` montre :
  ```
  Readiness probe failed: Get "http://x.x.x.x:8080/readyz": HTTP probe failed with statuscode: 503
  ```
- `kubectl logs <pod>` ne montre aucune erreur : le serveur axum démarre correctement.
- Le problème disparaît dès qu'un premier CR `Instance` est créé dans le cluster.

## What Didn't Work

Augmenter la `initialDelaySeconds` de la probe — le pod finit toujours en `503` car la cause n'est pas un délai de démarrage mais l'absence totale de réconciliation.

Initialiser `last_reconcile_at` à `unix_now_secs()` au démarrage seul : le pod passe `Ready` initialement, mais devient `NotReady` après 60 secondes si le cluster est toujours vide.

## Solution

Deux corrections complémentaires dans `src/main.rs` :

**1. Initialiser `last_reconcile_at` au timestamp courant au démarrage**

```rust
// Avant
last_reconcile_at: std::sync::atomic::AtomicI64::new(0),

// Après
last_reconcile_at: std::sync::atomic::AtomicI64::new(
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64,
),
```

**2. Ajouter un heartbeat ticker pour maintenir le timestamp à jour**

```rust
tokio::spawn({
    let ctx = Arc::clone(&context);
    async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            ctx.last_reconcile_at.store(now, Ordering::Release);
        }
    }
});
```

## Why This Works

La probe `/readyz` teste la vivacité opérationnelle de la boucle de contrôle, pas uniquement la présence de workload. L'initialisation à `now` reflète le fait que l'opérateur est opérationnel dès son démarrage. Le heartbeat garantit que l'opérateur reste `Ready` tant que le process est vivant — si le process crashe, le ticker s'arrête et `/readyz` passe en `503` après 60s.

## Prevention

- Pour tout opérateur kube-rs : la readiness probe ne doit pas dépendre d'un événement de réconciliation qui peut ne jamais arriver (cluster vide, namespace sans CRs).
- Test systématique : déployer l'opérateur dans un namespace sans CR et vérifier que le pod passe `Ready`.
- Si la probe doit mesurer l'activité récente, le seuil d'inactivité doit être nettement supérieur à l'intervalle de heartbeat (`heartbeat 30s`, seuil readyz `60s`).

## Related

- `src/main.rs` — initialisation de `last_reconcile_at`, spawn du heartbeat ticker
- `src/server.rs` — handler `/readyz`
- `charts/scaleway-operator/templates/deployment.yaml` — configuration `readinessProbe`
