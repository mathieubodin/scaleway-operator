# scaleway-operator-crds

Helm chart containing the Custom Resource Definitions (CRDs) for the Scaleway Kubernetes Operator.

## Installation

**Install this chart before installing `scaleway-operator`.**

```bash
helm install scaleway-operator-crds \
  oci://ghcr.io/mathieubodin/charts/scaleway-operator-crds \
  --version 0.1.0
```

## CRDs included

- `instances.scaleway.mathieubodin.io` (Namespaced) — reconciled
- `namespaceroles.scaleway.mathieubodin.io` (Cluster) — reconciled

## Notes

CRDs in this chart carry the annotation `helm.sh/resource-policy: keep`, which means they **survive `helm uninstall`**. This is intentional — deleting CRDs would destroy all custom resources in the cluster. To remove CRDs, delete them manually:

```bash
kubectl delete crd instances.scaleway.mathieubodin.io namespaceroles.scaleway.mathieubodin.io
```

## Upgrading

CRD upgrades require running `helm upgrade`:

```bash
helm upgrade scaleway-operator-crds \
  oci://ghcr.io/mathieubodin/charts/scaleway-operator-crds \
  --version <new-version>
```

Unlike the `crds/` directory pattern, CRDs in this chart are in `templates/` and **will be upgraded** on `helm upgrade`.
