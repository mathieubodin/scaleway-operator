---
title: "Kubernetes CRD API group must use a domain you own — not the upstream vendor's"
date: 2026-05-08
last_updated: 2026-05-08
category: docs/solutions/conventions/
module: resources
problem_type: convention
component: tooling
severity: high
applies_when:
  - Writing the first kube-rs custom resource type for a third-party service
  - Forking or adapting an existing operator from another author
  - Writing annotation key constants or finalizer strings in a Kubernetes operator
  - Wrapping a named cloud provider (Scaleway, AWS, GCP, Hetzner, etc.)
  - Setting metadata labels on CRD objects (label key prefix must also be your domain)
tags:
  - kubernetes
  - crd
  - api-group
  - kube-rs
  - naming
  - operator
  - convention
  - labels
---

# Kubernetes CRD API group must use a domain you own — not the upstream vendor's

## Context

When building a Kubernetes operator that wraps a third-party cloud provider, the natural reflex is to use the provider's domain as the API group — for example, `scaleway.io` for a Scaleway operator. This feels coherent: the CRD kinds are named after Scaleway concepts (Instance, Project, LoadBalancer), and the provider's domain appears everywhere in the documentation.

The problem is that `scaleway.io` is a domain owned by Scaleway SAS. Using it in an API group means every `CustomResourceDefinition`, every namespace annotation, and every finalizer string in etcd will appear to have been issued by Scaleway itself. A cluster admin seeing `scaleway.io/project-id` on a namespace assumes it was placed by official Scaleway tooling.

This was discovered during the PR review of the `scaleway-operator` Helm charts: the CRD group was `scaleway.io` in all Helm templates, Rust macro attributes, k8s manifests, RBAC rules, annotation constants, finalizer strings, and test assertions. The rename to `scaleway.mathieubodin.io` touched 25+ files.

## Guidance

**The rule:** the Kubernetes API group for a community or personal operator must use a domain the author controls. The provider name may appear in the subdomain (e.g., `scaleway.mathieubodin.io`), but the root domain must be yours.

**In kube-rs, the API group is declared once per resource type** via the `#[kube(group = "...")]` macro in `src/resources.rs`. This is the ground truth. Every other occurrence in the codebase — annotation key constants, finalizer strings, Helm chart CRDs, raw k8s manifests, RBAC `apiGroups`, test assertions — must match it exactly. Never scatter the domain string inline.

Declare annotation and finalizer strings as typed constants in a single file so the domain is never duplicated:

```rust
// src/context.rs — one source of truth for annotation key
const SCALEWAY_PROJECT_ANNOTATION: &str = "scaleway.mathieubodin.io/project-id";

// src/reconcilers.rs — one source of truth for finalizer
const INSTANCE_FINALIZER: &str = "scaleway.mathieubodin.io/instance-finalizer";
```

**Systematic rename methodology — do it in one pass:**

```bash
# Step 1: audit full scope before touching anything
grep -rn "scaleway\.io" \
  --include="*.rs" --include="*.yaml" --include="*.toml" --include="*.md" .

# Step 2: replace_all in one parallel batch (Edit tool with replace_all: true per file)
# Never rename file-by-file interactively — partial renames leave the repo inconsistent

# Step 3: verify no stragglers remain (output should be empty)
grep -rn "scaleway\.io" \
  --include="*.rs" --include="*.yaml" --include="*.toml" --include="*.md" . \
  | grep -v "scaleway\.mathieubodin\.io"
```

## Why This Matters

**The rename is cheap upfront and expensive after production deployments.** API group strings are stored in etcd as part of CRD names (`instances.scaleway.io`). Changing them after live custom resources exist requires creating new CRDs and migrating every existing resource — with downtime. One grep pass before the first commit costs nothing.

**It misleads cluster operators.** `scaleway.io/project-id` on a namespace reads as an annotation placed by official Scaleway tooling. Debugging, RBAC policies, and support escalations go wrong as a consequence.

**It is a forward compatibility hazard.** If Scaleway ships an official operator, its API group will naturally be `scaleway.io`. The community operator's CRDs would collide with no way to distinguish them.

**It affects every layer of the stack.** In a kube-rs project the scope of a domain rename is wider than expected:

| Layer | Files affected |
|-------|---------------|
| Rust macro | `src/resources.rs` — `#[kube(group = "...")]` on every CRD struct |
| Rust constants | `src/context.rs`, `src/reconcilers.rs` — annotation key, finalizer string, error messages |
| Helm charts | CRD templates (`group:`, `name:`), `clusterrole.yaml`, `namespace-bootstrap.yaml`, `NOTES.txt`, `Chart.yaml` (artifacthub annotations) |
| k8s/ manifests | CRD files, `deployment.yaml` (RBAC), examples, test fixtures |
| CRD metadata labels | Label keys on CRD objects — e.g., `io.scaleway.k8s.crd.schema.version` → `io.mathieubodin.scaleway.k8s.crd.schema.version` |
| Tests | `tests/integration.rs` — assertion strings like `.contains("scaleway.io/project-id")` |
| Docs | `README.md`, `CLAUDE.md`, `docs/solutions/` |

## When to Apply

- When writing the first `#[kube(group = "...")]` in any operator — set the domain before writing any other file.
- When forking an operator: verify the API group domain is not someone else's before your first commit.
- When declaring annotation key constants or finalizer strings: the domain prefix must match `#[kube(group)]`.
- When wrapping a named cloud provider: the provider name may appear in the subdomain (`scaleway.mathieubodin.io`) but the root domain must be yours.

## Examples

**`src/resources.rs` — ground truth (before / after)**

```rust
// Before — uses Scaleway's domain
#[kube(group = "scaleway.io", version = "v1", kind = "Instance")]
pub struct InstanceSpec { ... }

// After — uses author's domain
#[kube(group = "scaleway.mathieubodin.io", version = "v1", kind = "Instance")]
pub struct InstanceSpec { ... }
```

**`src/context.rs` — annotation constant**

```rust
// Before
const SCALEWAY_PROJECT_ANNOTATION: &str = "scaleway.io/project-id";

// After
const SCALEWAY_PROJECT_ANNOTATION: &str = "scaleway.mathieubodin.io/project-id";
```

**Helm CRD template**

```yaml
# Before
metadata:
  name: instances.scaleway.io
spec:
  group: scaleway.io

# After
metadata:
  name: instances.scaleway.mathieubodin.io
spec:
  group: scaleway.mathieubodin.io
```

**RBAC ClusterRole**

```yaml
# Before
- apiGroups: ["scaleway.io"]
  resources: ["instances", "instances/status"]
  verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]

# After
- apiGroups: ["scaleway.mathieubodin.io"]
  resources: ["instances", "instances/status"]
  verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
```

## Related

- `docs/solutions/architecture-patterns/namespacerole-namespace-annotation-scaleway-multiproject-2026-05-03.md` — Architecture using `scaleway.mathieubodin.io` as the API group throughout; concrete reference for the post-rename state of the codebase.
