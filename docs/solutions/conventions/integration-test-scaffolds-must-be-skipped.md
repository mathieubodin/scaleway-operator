---
title: "Un test d'intégration en scaffold doit être ajouté à la liste --skip du script kind"
date: 2026-10-09
category: docs/solutions/conventions/
module: integration-tests
problem_type: convention
component: testing_framework
severity: high
applies_when:
  - "On ajoute dans tests/integration.rs un test incomplet qui se termine par unimplemented!() ou todo!()"
  - "On marque un test #[ignore] en pensant l'exclure de la CI"
  - "On relit une PR dont seuls lint et unit-tests tournent"
tags:
  - integration-tests
  - kind
  - cargo-test
  - ignore-attribute
  - ci
  - scaffold
---

# Un test d'intégration en scaffold doit être ajouté à la liste --skip du script kind

## Context

Dans ce dépôt, `#[ignore]` ne sert pas à exclure un test : il sert à le réserver au cluster kind.
`scripts/test-integration-kind.sh` lance `cargo test --test integration -- --ignored`, donc tous les tests
marqués `#[ignore]` sont exécutés par `make test-integration-kind` et `make coverage-kind-lcov`.

La PR #113 a ajouté `test_scalewaysecret_create_with_mock_scaleway_writes_status`, un scaffold marqué
`#[ignore = "see #118 …"]` et terminé par `unimplemented!()`. L'auteur le croyait exclu. Il aurait paniqué
dans le job d'intégration dès le merge sur `main`, et bloqué la publication qui en dépend.

La CI de la PR ne pouvait pas le voir : `pr.yml` ne lance que `lint` et `unit-tests`, où les tests
`#[ignore]` sont bien ignorés. Le défaut a été trouvé en review, en lisant le script et non le seul test.

## Guidance

Un test d'intégration incomplet doit suivre l'une de ces deux règles :

- l'ajouter à la liste `--skip` des deux commandes de `scripts/test-integration-kind.sh`, avec l'issue de suivi
  dans le motif du `#[ignore]` ;
- ou sortir tôt sans paniquer tant que sa précondition manque, comme le fait le test LoadBalancer live avec
  la variable `SCALEWAY_LB_LIVE_TEST`.

```bash
cargo test --test integration -- --ignored \
    --skip test_loadbalancer_create_sync_delete \
    --skip test_scalewaysecret_create_with_mock_scaleway_writes_status
```

Avant de merger une PR qui touche `tests/integration.rs`, lancer `make test-integration-kind` en local :
c'est le seul contrôle qui exécute ces tests avant `main`.

## Why This Matters

Un scaffold qui panique ne casse rien sur la PR et casse tout après le merge. Le coût est reporté sur
`main`, là où il bloque aussi la release. Le motif du `#[ignore]` donne une fausse assurance : il décrit
une intention d'exclusion que le script ne respecte pas.

## When to Apply

- À chaque ajout d'un test dans `tests/integration.rs` qui n'est pas encore complet.
- En review, dès qu'un test contient `unimplemented!()`, `todo!()` ou un `TODO` à la place de ses assertions.
- Quand on retire un `--skip` : vérifier que le test passe réellement sur kind.

## Examples

Liste des tests que le job d'intégration va réellement exécuter :

```bash
cargo test --test integration -- --ignored \
    --skip test_loadbalancer_create_sync_delete \
    --skip test_scalewaysecret_create_with_mock_scaleway_writes_status --list
```

Recherche des scaffolds restants :

```bash
grep -n 'unimplemented!\|todo!' tests/integration.rs
```

## Related

- `docs/solutions/tooling-decisions/kind-ephemeral-cluster-integration-tests-2026-05-23.md`
- `docs/solutions/developer-experience/coverage-kind-integration-2026-06-04.md`
- Issues #118 et #119 : scaffolds à compléter
