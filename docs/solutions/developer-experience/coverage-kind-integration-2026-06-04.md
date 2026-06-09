---
title: Mesurer le coverage en combinant tests unitaires et tests d'intégration kind
date: 2026-06-04
category: docs/solutions/developer-experience
module: coverage
problem_type: developer_experience
component: testing_framework
severity: medium
applies_when:
  - Les tests d'intégration nécessitent un cluster Kubernetes éphémère (kind)
  - On veut un rapport de coverage fusionnant unit tests et integration tests
  - cargo llvm-cov est utilisé comme outil de mesure
tags:
  - coverage
  - cargo-llvm-cov
  - kind
  - integration-tests
  - makefile
---

# Mesurer le coverage en combinant tests unitaires et tests d'intégration kind

## Context

Les targets `make coverage` existantes (`coverage`, `coverage-json`, `coverage-text`) utilisaient
`cargo llvm-cov --lib --tests` — ce qui instrumente et exécute uniquement les tests unitaires.
Les tests d'intégration (dans `tests/integration.rs`) sont marqués `#[ignore]` et nécessitent
un cluster kind éphémère : ils n'étaient donc jamais comptabilisés dans le coverage.

Le résultat : les chemins de code exercés uniquement par les réconciliateurs lors de vrais
appels Kubernetes (ajout de finalizer, gestion de statut, suppression) n'apparaissaient pas
dans les rapports.

## Guidance

`cargo llvm-cov` supporte un workflow en deux passes avec `--no-report` :

```
# Passe 1 — tests unitaires instrumentés (accumule les profraw, pas de rapport)
cargo llvm-cov --no-report --lib --tests

# Passe 2 — tests d'intégration instrumentés dans kind (accumule les profraw)
KUBECONFIG=... cargo llvm-cov --no-report --test integration \
  -- --ignored --skip test_loadbalancer_create_sync_delete

# Fusion — génère le rapport depuis tous les profraw accumulés
cargo llvm-cov report          # tableau de synthèse terminal
cargo llvm-cov report --html   # rapport HTML interactif
cargo llvm-cov report --json   # JSON pour CI
```

Les profraw des deux passes sont fusionnés automatiquement lors du `report`.

### Structure Makefile

```makefile
CARGO_COV_FILTER = grep -vE "^   (Compiling|Checking)|^    Finished|^     Running|^running [0-9]|^[.i]|^info: cargo-llvm-cov|^test [^ ]+ \.\.\. (ok|ignored)|^$$"

coverage-kind-text: check-llvm-cov check-kind check-docker check-helm
 @echo "[1/4] Nettoyage des données de coverage..."
 @cargo llvm-cov clean > /dev/null 2>&1 || true
 @echo "[2/4] Tests unitaires..."
 @bash -c 'set -o pipefail; cargo llvm-cov --no-report --lib --tests 2>&1 | $(CARGO_COV_FILTER)'
 @echo "[3/4] Tests d'intégration (cluster kind éphémère)..."
 @bash scripts/test-integration-kind.sh --coverage
 @echo "[4/4] Synthèse de coverage..."
 @echo ""
 @cargo llvm-cov report
```

### Flag `--coverage` dans le script kind

```bash
# scripts/test-integration-kind.sh
COVERAGE=false
for arg in "$@"; do
    case "$arg" in
        --coverage) COVERAGE=true ;;
    esac
done

if [ "$COVERAGE" = true ]; then
    export KUBECONFIG="$KIND_KUBECONFIG"
    bash -c "set -o pipefail; cargo llvm-cov --no-report --test integration \
      -- --ignored --skip test_loadbalancer_create_sync_delete 2>&1 | $CARGO_COV_FILTER"
else
    export KUBECONFIG="$KIND_KUBECONFIG"
    bash -c "set -o pipefail; cargo test --test integration \
      -- --ignored --skip test_loadbalancer_create_sync_delete 2>&1 | $CARGO_COV_FILTER"
fi
```

## Why This Matters

Sans fusion des deux passes, les fonctions de réconciliation qui ne s'expriment qu'avec
un vrai cluster Kubernetes (ajout de finalizer, sync de statut, suppression) apparaissent
comme non couvertes alors qu'elles sont bien testées. Le résultat combiné représente
la couverture réelle du projet (~81% lignes vs ~60% sans les tests d'intégration).

## When to Apply

- Avant chaque merge sur `main` quand on veut le chiffre de coverage mesuré (checklist pré-merge)
- En CI avec un job GitHub Actions disposant de Docker/kind
- Pour identifier les chemins de réconciliation non couverts (`reconcilers.rs`)

## Examples

### Pièges rencontrés lors de l'implémentation

**`cargo llvm-cov report --text` ≠ tableau de synthèse**

`--text` affiche les annotations source ligne par ligne (4000+ lignes). Pour le tableau
de synthèse par fichier, utiliser `cargo llvm-cov report` sans flag de format.

**`-q` et `-- --quiet` en conflit**

```
# ERREUR : Option 'quiet' given more than once
cargo llvm-cov --no-report -q --lib --tests -- --quiet
```

`cargo llvm-cov -q` passe `-q` à `cargo test`, qui est équivalent à `-- --quiet` pour
le test binary — les deux ensemble provoquent un conflit. De plus, `-q` seul ne supprime
pas la sortie du test runner (dots, "running N tests") avec cargo llvm-cov.

Solution : filtrer la sortie via `CARGO_COV_FILTER` dans un sous-shell bash avec `pipefail`
pour préserver le code de retour de cargo :

```bash
bash -c 'set -o pipefail; cargo llvm-cov --no-report --lib --tests 2>&1 | grep -vE "..."'
```

**`cargo llvm-cov clean` supprime `target/llvm-cov/`**

Appeler `mkdir -p $(COVERAGE_DIR)` APRÈS `cargo llvm-cov clean`, pas avant, pour les
targets qui écrivent dans ce répertoire (JSON).

**Infrastructure kind/helm : rediriger vers un log**

```bash
KIND_LOG=".kube/kind-setup.log"
kind create cluster ... > "$KIND_LOG" 2>&1 \
    || { echo "ERREUR kind create — voir $KIND_LOG" >&2; exit 1; }
helm upgrade --install ... >> "$KIND_LOG" 2>&1 \
    || { echo "ERREUR helm upgrade — voir $KIND_LOG" >&2; exit 1; }
```

Le log est disponible pour diagnostic sans polluer la sortie standard.

## Related

- `Makefile` — targets `coverage-kind`, `coverage-kind-json`, `coverage-kind-text`
- `scripts/test-integration-kind.sh` — flag `--coverage`
- `docs/solutions/` — autres patterns de test de cet opérateur
