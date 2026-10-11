---
title: "Un reconcile parti d'un cache en retard rejoue une mutation externe déjà faite"
date: 2026-10-11
category: docs/solutions/logic-errors/
module: scaleway-secret
problem_type: logic_error
component: background_job
severity: medium
symptoms:
  - "Deux versions identiques du secret Scaleway après une création nominale, la première désactivée par la seconde"
  - "Le reconcile déclenché par le patch de status préliminaire `Syncing` décide `PushNewVersion` alors que l'API server porte déjà `Synced`"
  - "Un CR reste en `sync_state: Error` après une erreur passagère alors que la source est de nouveau lisible et que rien n'est à pousser"
root_cause: async_timing
resolution_type: code_fix
related_components:
  - "testing_framework"
tags:
  - "kube-rs"
  - "kubernetes-operator"
  - "reconciler"
  - "stale-cache"
  - "resource-version"
  - "idempotence"
  - "status-subresource"
  - "scaleway-secret-manager"
---

# Un reconcile parti d'un cache en retard rejoue une mutation externe déjà faite

## Problem

Le reconciler `ScalewaySecret` écrit un status préliminaire `Syncing` avant de pousser la première version, pour
réserver le `scaleway_id` (issue #117). Ce patch déclenche lui-même un reconcile. Si celui-ci reçoit du cache du
controller un CR qui porte encore le status préliminaire, il décide `PushNewVersion` et pousse une seconde version
identique dans Scaleway Secret Manager.

## Symptoms

- Deux versions créées coup sur coup pour un secret qui n'a pas changé : `status.current_version` vaut 2 juste après
  une création, alors que le Secret source n'a pas tourné.
- Aucune erreur dans les logs ni dans le status : les deux reconciles se terminent en `Synced`.
- Pas de fuite de version active, parce que chaque envoi passe `disable_previous` à Scaleway. Le coût est une
  version et un appel API de trop par création.

Niveau de preuve : le défaut n'a pas été observé en production. Il a été relevé par la review de la PR #113, puis
confirmé par lecture du code. Le test d'intégration `stale_cached_cr_does_not_push_a_second_version` construit le
scénario (objet figé à l'état `Syncing`, API server déjà en `Synced`) et vérifie qu'aucun appel Scaleway ne part.
Il échoue quand la garde est neutralisée et passe quand elle est rétablie.

## What Didn't Work

Rien n'a été essayé puis abandonné après échec. Deux choix de conception ont été écartés pendant la correction :

- **Redécider sur le status frais.** L'issue #168 proposait de relire le CR et de relancer la décision sur ce status.
  Cela oblige à reconstruire tout l'input de décision à partir d'un second objet, au milieu d'une branche d'I/O.
  Rendre la main et laisser le controller relancer un reconcile sur un cache à jour arrive au même résultat avec
  deux lignes.
- **Garde limitée à `PushNewVersion`.** C'était le périmètre de l'issue. La branche `CreateAndSyncSecret` a le même
  défaut : un cache qui ne voit pas encore le `scaleway_id` relance une création, adopte le secret par ses tags et
  pousse une version en double. La garde couvre donc les deux branches.

## Solution

Avant toute mutation Scaleway, le reconciler relit le CR sur l'API server et compare son `resourceVersion` à celui
de l'objet reçu du cache. S'ils diffèrent, il ne pousse rien et repasse cinq secondes plus tard.

```rust
/// Délai avant de relancer un reconcile parti d'un cache en retard sur l'API server.
const STALE_CACHE_REQUEUE: Duration = Duration::from_secs(5);

/// Vrai si le CR reçu du cache du controller est celui que porte l'API server.
///
/// À vérifier avant de pousser une version : un status écrit par le reconcile précédent
/// mais pas encore vu par le cache ferait pousser une seconde version identique.
async fn is_cached_cr_current(
    secret_cr: &ScalewaySecret,
    api: &Api<ScalewaySecret>,
) -> Result<bool> {
    let live = api.get(&secret_cr.name_any()).await?;
    Ok(live.metadata.resource_version == secret_cr.metadata.resource_version)
}
```

La garde ouvre les deux branches qui écrivent dans Scaleway, `CreateAndSyncSecret` et `PushNewVersion` :

```rust
if !is_cached_cr_current(&secret_cr, &api).await? {
    return Ok(Action::requeue(STALE_CACHE_REQUEUE));
}
```

Avant la correction, ces branches agissaient directement sur le status de l'objet reçu en argument.

Le test d'intégration fige l'objet tel que le verrait un cache en retard, puis fait avancer l'API server :

```rust
// Ce que voit le cache du controller : le status préliminaire de la création.
let mut reserved = synced_status("sec-stale", 1, &source_rv);
reserved.current_version = None;
reserved.last_synced_resource_version = None;
reserved.sync_state = "Syncing".to_string();
fixture.set_scaleway_secret_status(&name, reserved).await;
let cached = fixture.get_scaleway_secret(&name).await;
// Ce que porte déjà l'API server : la création s'est terminée.
let mut synced = synced_status("sec-stale", 1, &source_rv);
synced.observed_generation = created.metadata.generation;
fixture.set_scaleway_secret_status(&name, synced).await;

let result =
    scaleway_operator::reconcilers::reconcile_scaleway_secret(Arc::new(cached), ctx.clone())
        .await;
```

Il attend zéro `GET` et zéro `POST` sur le mock Scaleway, et un status resté à `Synced`, version 1.

Limites acceptées :

- un `GET` de plus sur l'API server par création et par rotation ; le chemin sans mutation (`AlreadySynced`) n'est
  pas touché ;
- la comparaison porte sur le `resourceVersion` de tout l'objet : une modification du CR sans rapport avec le
  status (label, annotation, autre controller) entre la lecture du cache et la garde provoque aussi un requeue de
  cinq secondes. La mutation est retardée, jamais perdue.

## Why This Works

Le reconciler reçoit son CR du cache du controller, alimenté par un watch, et non d'une lecture directe. Ce cache
est cohérent à terme : après un `patch_status`, rien ne garantit que l'objet servi au reconcile suivant contient
déjà ce patch.

La séquence fautive est la suivante. Le premier reconcile écrit le status préliminaire (`scaleway_id` présent,
`last_synced_resource_version` absent), pousse la version 1, puis écrit le status final. Le patch préliminaire a
mis un reconcile en file. Quand il s'exécute, si le cache n'a vu que le patch préliminaire, la décision lit
`scaleway_id` présent et `last_synced_resource_version` différent du `resourceVersion` du Secret source : c'est
exactement la signature d'une rotation, donc `PushNewVersion`.

La décision est juste pour l'état qu'elle voit ; c'est l'état qui est périmé. Un `get` sur l'API server donne un
état au moins aussi récent que le dernier patch du reconciler lui-même. Si son `resourceVersion` diffère de celui
du cache, la décision a été prise sur une vue dépassée, et la seule action sûre est de ne rien muter. Le watch
finit par livrer l'objet à jour, et le reconcile suivant décide `AlreadySynced`.

Le status préliminaire de #117 reste nécessaire : il protège du cas inverse, où la version est poussée mais le
status final n'est jamais écrit. Les deux gardes sont complémentaires.

## Prevention

- **Relire avant de muter un système externe.** Un reconciler kube-rs qui écrit son propre status puis agit sur
  une API externe non idempotente doit vérifier, juste avant l'effet, que son objet est celui de l'API server. La
  règle vaut pour les futures ressources qui réservent un identifiant dans le status avant de créer (RDB, Bucket,
  RedisCluster).
- **Placer la garde sur toutes les branches à effet, pas sur celle du ticket.** Le défaut signalé visait
  `PushNewVersion` ; `CreateAndSyncSecret` avait la même faille par un autre chemin.
- **Tester avec un objet figé.** Les helpers de test relisent le CR avant chaque reconcile, ce qui masque ce genre
  de défaut. Pour le reproduire, capturer l'objet, faire avancer l'API server, puis appeler le reconciler avec
  l'objet capturé, comme dans `stale_cached_cr_does_not_push_a_second_version`.
- **Compter les appels externes dans les tests.** Un mock `expect(0)` sur toutes les méthodes détecte un effet en
  trop que le status final ne montre pas.
- **Effacer un `Error` dans la branche « rien à faire ».** Défaut voisin corrigé dans le même commit (issue #164) :
  une branche d'erreur posait `Error` dans le status, et la branche `AlreadySynced` ne le réécrivait pas. Après
  une erreur passagère, le CR restait en `Error` alors que tout était synchronisé. Toute branche qui conclut que
  l'état voulu est atteint doit écrire le status correspondant, sans écrire quand il est déjà à jour :

  ```rust
  fn build_already_synced_status(
      current_status: &ScalewaySecretStatus,
      generation: Option<i64>,
  ) -> Option<ScalewaySecretStatus> {
      let mut status = current_status.clone();
      status.sync_state = "Synced".to_string();
      status.error_message = None;
      if status.observed_generation.is_none() {
          status.observed_generation = generation;
      }
      (status != *current_status).then_some(status)
  }
  ```

  Le retour `None` évite un `patch_status` à chaque requeue de trente secondes, qui relancerait lui-même un
  reconcile.

## Related Issues

- #168 : double push après une création nominale (ce document).
- #164 : status bloqué en `Error` après une erreur passagère.
- #117 : status préliminaire `Syncing`, qui réserve le `scaleway_id` avant le premier envoi.
- #129 : issue parente, findings de correction de la review de #113.
- Commit `9db427e`.
- [Détection de rotation par resourceVersion](../architecture-patterns/scaleway-secret-resource-version-rotation-detection-2026-06-13.md) :
  autre origine de versions identiques superflues. Ce document-là parle du `resourceVersion` du Secret source,
  celui-ci du `resourceVersion` du CR `ScalewaySecret`.
