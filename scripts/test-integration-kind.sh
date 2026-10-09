#!/usr/bin/env bash
set -euo pipefail

CLUSTER_NAME="scaleway-operator-test"
KIND_KUBECONFIG=".kube/kind-config"
KIND_LOG=".kube/kind-setup.log"
COVERAGE=false

for arg in "$@"; do
    case "$arg" in
        --coverage) COVERAGE=true ;;
    esac
done

CHART_CRDS_VERSION=$(grep '^version:' charts/scaleway-operator-crds/Chart.yaml | awk '{print $2}')
if [ -z "$CHART_CRDS_VERSION" ]; then
    echo "ERROR: impossible de lire la version CRDs depuis charts/scaleway-operator-crds/Chart.yaml" >&2
    exit 1
fi

cleanup() {
    kind delete cluster --name "$CLUSTER_NAME" > /dev/null 2>&1 || true
    rm -f "$KIND_KUBECONFIG"
}
trap cleanup EXIT

mkdir -p .kube target/charts

echo "[kind] Création du cluster ${CLUSTER_NAME}..."
kind create cluster --name "$CLUSTER_NAME" --kubeconfig "$KIND_KUBECONFIG" > "$KIND_LOG" 2>&1 \
    || { echo "ERREUR kind create — voir $KIND_LOG" >&2; exit 1; }

echo "[kind] Déploiement des CRDs v${CHART_CRDS_VERSION}..."
helm package charts/scaleway-operator-crds/ --destination target/charts/ >> "$KIND_LOG" 2>&1
helm upgrade --install scaleway-operator-crds \
    "target/charts/scaleway-operator-crds-${CHART_CRDS_VERSION}.tgz" \
    --kubeconfig "$KIND_KUBECONFIG" \
    --namespace scaleway-system \
    --create-namespace \
    --wait >> "$KIND_LOG" 2>&1 \
    || { echo "ERREUR helm upgrade — voir $KIND_LOG" >&2; exit 1; }

echo "[kind] Application des fixtures..."
kubectl --kubeconfig="$KIND_KUBECONFIG" apply -f k8s/test-fixtures.yaml >> "$KIND_LOG" 2>&1

echo "[kind] Tests d'intégration..."
CARGO_COV_FILTER='grep -vE "^   (Compiling|Checking)|^    Finished|^     Running|^running [0-9]|^[.i]|^info: cargo-llvm-cov|^test [^ ]+ \.\.\. (ok|ignored)|^$"'

if [ "$COVERAGE" = true ]; then
    export KUBECONFIG="$KIND_KUBECONFIG"
    bash -c "set -o pipefail; cargo llvm-cov --no-report --test integration -- --ignored --skip test_loadbalancer_create_sync_delete 2>&1 | $CARGO_COV_FILTER"
else
    export KUBECONFIG="$KIND_KUBECONFIG"
    bash -c "set -o pipefail; cargo test --test integration -- --ignored --skip test_loadbalancer_create_sync_delete 2>&1 | $CARGO_COV_FILTER"
fi
