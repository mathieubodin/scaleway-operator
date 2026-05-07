# scaleway-operator

Helm chart for the Scaleway Kubernetes Operator — reconciles Scaleway cloud resources (Instances, NamespaceRoles) from Kubernetes CRs.

## Prerequisites

- Kubernetes 1.35+
- Helm 3.8+ (OCI registry support required)
- `scaleway-operator-crds` chart installed first

## Installation

### Step 1: Install CRDs

```bash
helm install scaleway-operator-crds \
  oci://ghcr.io/mathieubodin/charts/scaleway-operator-crds \
  --version 0.1.0
```

### Step 2: Install the operator

```bash
helm install scaleway-operator \
  oci://ghcr.io/mathieubodin/charts/scaleway-operator \
  --version 0.1.0 \
  --namespace scaleway-system --create-namespace \
  --set scaleway.token=<YOUR_TOKEN> \
  --set scaleway.organizationId=<YOUR_ORG_ID>
```

Or using an existing Secret:

```bash
helm install scaleway-operator \
  oci://ghcr.io/mathieubodin/charts/scaleway-operator \
  --version 0.1.0 \
  --namespace scaleway-system --create-namespace \
  --set scaleway.existingSecret=my-scaleway-credentials
```

## Per-namespace prerequisites

Each namespace hosting `Instance` resources requires:

1. The annotation `scaleway.io/project-id` on the namespace
2. A cluster-wide `NamespaceRole` resource whose name matches the namespace
3. A Secret `scaleway-ns-creds-<namespace>` in `scaleway-system` (provisioned by admin)

Use `namespaceBootstrap` to automate steps 1 and 2:

```yaml
namespaceBootstrap:
  enabled: true
  namespaces:
    - name: production
      projectId: "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
    - name: staging
      projectId: "yyyyyyyy-yyyy-yyyy-yyyy-yyyyyyyyyyyy"
```

## Verify installation

```bash
kubectl -n scaleway-system rollout status deployment/scaleway-operator
kubectl -n scaleway-system logs -l app.kubernetes.io/name=scaleway-operator
```

## Image signature verification

Images are signed with cosign (keyless, Sigstore):

```bash
cosign verify ghcr.io/mathieubodin/scaleway-operator:<version> \
  --certificate-identity-regexp "^https://github.com/mathieubodin/scaleway-operator/.github/workflows/release.yml@refs/tags/.*" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com"
```
