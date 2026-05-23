#!/usr/bin/env bash
set -euo pipefail

CLUSTER_NAME="scaleway-operator-test"
KIND_KUBECONFIG=".kube/kind-config"

CHART_CRDS_VERSION=$(grep '^version:' charts/scaleway-operator-crds/Chart.yaml | awk '{print $2}')
if [ -z "$CHART_CRDS_VERSION" ]; then
    echo "ERROR: impossible de lire la version CRDs depuis charts/scaleway-operator-crds/Chart.yaml" >&2
    exit 1
fi

cleanup() {
    echo "--- Suppression du cluster kind ${CLUSTER_NAME} ---"
    kind delete cluster --name "$CLUSTER_NAME" 2>/dev/null || true
    rm -f "$KIND_KUBECONFIG"
}
trap cleanup EXIT

mkdir -p .kube target/charts

echo "--- Création du cluster kind ${CLUSTER_NAME} ---"
kind create cluster --name "$CLUSTER_NAME" --kubeconfig "$KIND_KUBECONFIG"

echo "--- Déploiement des CRDs (v${CHART_CRDS_VERSION}) ---"
helm package charts/scaleway-operator-crds/ --destination target/charts/ --quiet
helm upgrade --install scaleway-operator-crds \
    "target/charts/scaleway-operator-crds-${CHART_CRDS_VERSION}.tgz" \
    --kubeconfig "$KIND_KUBECONFIG" \
    --namespace scaleway-system \
    --create-namespace \
    --wait

echo "--- Application des fixtures de test ---"
kubectl --kubeconfig="$KIND_KUBECONFIG" apply -f k8s/test-fixtures.yaml

echo "--- Exécution des tests d'intégration ---"
KUBECONFIG="$KIND_KUBECONFIG" cargo test --test integration -- --ignored
