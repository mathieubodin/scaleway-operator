---
title: kind ephemeral cluster for kube-rs integration tests
date: 2026-05-23
category: docs/solutions/tooling-decisions
module: integration-tests
problem_type: tooling_decision
component: testing_framework
severity: medium
applies_when:
  - "integration tests require a real Kubernetes API server"
  - "CI runners have Docker but no pre-existing cluster"
  - "test suite uses kube-rs Client::try_default() to connect"
root_cause: missing_tooling
resolution_type: tooling_addition
tags:
  - kind
  - kubernetes
  - integration-tests
  - ci
  - rust
  - kube-rs
  - helm
  - github-actions
---

# kind ephemeral cluster for kube-rs integration tests

## Context

Les tests d'intégration de `reconcile_instance` dans `tests/integration.rs` étaient tous marqués `#[ignore]`. Ils dépendaient d'un `kubectl proxy` tournant sur `http://127.0.0.1:8001` et d'une variable `KUBE_API_URL` injectée manuellement — un setup impossible à reproduire en CI et trop fragile pour le développement local. La couverture de `reconcilers.rs` stagnait à ~48% précisément parce que ces tests ne s'exécutaient jamais.

## Guidance

Remplacer le setup kubectl-proxy par un cluster **kind** (Kubernetes IN Docker) éphémère orchestré par un script shell dédié.

### Script shell (`scripts/test-integration-kind.sh`)

```bash
#!/usr/bin/env bash
set -euo pipefail

CLUSTER_NAME="scaleway-operator-test"
KIND_KUBECONFIG=".kube/kind-config"

cleanup() {
    kind delete cluster --name "$CLUSTER_NAME" 2>/dev/null || true
    rm -f "$KIND_KUBECONFIG"
}
trap cleanup EXIT   # EXIT, pas ERR — garantit le nettoyage sur succès ET échec

kind create cluster --name "$CLUSTER_NAME" --kubeconfig "$KIND_KUBECONFIG"

# Déployer les CRDs inline — NE PAS appeler $(MAKE) deploy-crds
# (sa target check-kubeconfig vérifie .kube/config qui n'existe pas ici)
helm package charts/scaleway-operator-crds/ --destination target/charts/ --quiet
helm upgrade --install scaleway-operator-crds \
    "target/charts/scaleway-operator-crds-${CHART_CRDS_VERSION}.tgz" \
    --kubeconfig "$KIND_KUBECONFIG" \
    --namespace scaleway-system \
    --create-namespace \
    --wait

kubectl --kubeconfig="$KIND_KUBECONFIG" apply -f k8s/test-fixtures.yaml

# Exporter KUBECONFIG avant cargo test — c'est ce que Client::try_default() lit
KUBECONFIG="$KIND_KUBECONFIG" cargo test --test integration -- --ignored
```

### Connexion Rust au cluster

`Client::try_default()` lit `KUBECONFIG` en priorité. Il suffit d'exporter la variable avant `cargo test` — pas besoin de modifier le code Rust ni de passer `--kubeconfig` à cargo.

### Fixtures de test

Les namespaces, NamespaceRoles et Secrets sont pré-créés par `k8s/test-fixtures.yaml` (appliqué par le script). Les tests ne créent et suppriment que des objets `Instance`. L'API Scaleway est mockée via `mockito` — aucune credential réelle requise.

### Gate CI

```yaml
integration-tests:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@<sha>
    - name: Install kind
      run: curl -sSLo /usr/local/bin/kind "https://github.com/kubernetes-sigs/kind/releases/download/v0.31.0/kind-linux-amd64" && chmod +x /usr/local/bin/kind
    - uses: dtolnay/rust-toolchain@<sha>
    - uses: actions/cache@<sha>
    - uses: azure/setup-helm@<sha>
    - run: make test-integration-kind

build-and-push:
  needs: [release-please, integration-tests]
  # result == 'success' explicite — évite le blocage silencieux si integration-tests est skippé
  if: ${{ needs.release-please.outputs.release_created == 'true' && needs.integration-tests.result == 'success' }}
```

### Toolchain

Ajouter `rust-toolchain.toml` à la racine pour que rustup gère la toolchain automatiquement :

```toml
[toolchain]
channel = "stable"
```

## Why This Matters

- **Tests exécutables sans infrastructure** : un contributeur avec Docker, kind, helm et kubectl peut lancer `make test-integration-kind` sans aucun cluster pré-existant.
- **Gate de release fiable** : `build-and-push` ne peut pas publier si un test d'intégration est rouge.
- **Cleanup garanti** : `trap EXIT` (pas `trap ERR`) s'exécute que les tests passent ou échouent — aucun cluster orphelin.
- **API Scaleway mockée** : mockito intercepte les appels HTTP dans les tests ; aucune credential Scaleway n'est transmise au cluster kind.

## When to Apply

- Opérateur kube-rs dont les tests d'intégration nécessitent un vrai API server Kubernetes
- CI `ubuntu-latest` (Docker disponible nativement)
- Tests qui utilisent `Client::try_default()` pour la connexion cluster

## Examples

**Avant** — tests inutilisables en CI :

```makefile
# Nécessitait kubectl proxy + KUBE_API_URL + cluster pré-existant
test-integration:
    KUBE_API_URL=$(KUBE_API_URL) cargo test --test integration -- --ignored
```

**Après** — cluster éphémère, zéro infrastructure préalable :

```makefile
test-integration-kind: check-cargo check-kind check-docker check-helm
    bash scripts/test-integration-kind.sh
```

## Points d'attention

- **Ne pas appeler `$(MAKE) deploy-crds`** depuis le script : la target a `check-kubeconfig` en prérequis, qui vérifie `.kube/config` — absent dans le contexte kind. Inliner les commandes helm avec `--kubeconfig` explicite.
- **`trap EXIT` et non `trap ERR`** : `ERR` ne se déclenche pas si une commande réussit avant l'erreur finale ; `EXIT` couvre tous les cas.
- **Prérequis à documenter** : kubectl est requis (appelé dans le script) mais souvent omis des listes de prérequis axées Rust/kind/helm. Le vérifier dans `make env-check`.

## Related

- `scripts/test-integration-kind.sh`
- `k8s/test-fixtures.yaml`
- `tests/integration.rs`
- `.github/workflows/release.yml` — job `integration-tests`
- PR #37 — implémentation initiale
- Issue #32 — motivation originale
