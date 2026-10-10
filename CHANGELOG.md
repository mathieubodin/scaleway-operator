# Changelog

User-facing changes to the operator binary and its container image. Chart changes are in `charts/*/CHANGELOG.md`.

## [0.1.15](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.14...scaleway-operator-v0.1.15) (2026-10-10)

### Bug Fixes

* **scaleway-secret:** push a new version when the `ScalewaySecret` spec changes, for instance the key read from the source Secret ([#127](https://github.com/mathieubodin/scaleway-operator/issues/127))
* **scaleway-secret:** pick up a fix made on the source Secret (opt-in label, missing key, RBAC) within 5 minutes, without touching the `ScalewaySecret` ([#128](https://github.com/mathieubodin/scaleway-operator/issues/128))
* **scaleway-secret:** keep a single enabled version in Scaleway Secret Manager, even when a status update is lost after a push ([#117](https://github.com/mathieubodin/scaleway-operator/issues/117))
* retry transient Kubernetes API errors when reading the namespace credentials, instead of waiting for a change of the resource ([#128](https://github.com/mathieubodin/scaleway-operator/issues/128))

### Upgrade notes

* Upgrade the `scaleway-operator-crds` chart to 0.1.14 before the operator: the `ScalewaySecret` status gains the `observed_generation` field.

## [0.1.14](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.13...scaleway-operator-v0.1.14) (2026-10-09)

### Features

* **scaleway-secret:** add the `ScalewaySecret` CRD, which syncs one key of a Kubernetes Secret to Scaleway Secret Manager and pushes a new version when the Secret changes ([#35](https://github.com/mathieubodin/scaleway-operator/issues/35))

### Upgrade notes

* The operator only reads a source Secret that opts in: it must carry the label `scaleway.mathieubodin.io/allow-operator-read: "true"` and the annotation `scaleway.mathieubodin.io/allowed-cr: "<namespace>/<name>"` naming the `ScalewaySecret` allowed to read it.
* Requires the `scaleway-operator-crds` chart 0.1.13 or later, and the per-namespace Role created by `namespaceBootstrap` in the `scaleway-operator` chart 0.1.13.

## [0.1.13](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.12...scaleway-operator-v0.1.13) (2026-06-12)

### Bug Fixes

* **image:** fix the multi-arch image build, which failed to add the musl targets under the pinned Rust toolchain

## [0.1.12](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.11...scaleway-operator-v0.1.12) (2026-06-12)

No user-facing changes.

## [0.1.11](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.10...scaleway-operator-v0.1.11) (2026-06-03)

No user-facing changes.

## [0.1.10](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.9...scaleway-operator-v0.1.10) (2026-05-29)

No user-facing changes.

## [0.1.9](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.8...scaleway-operator-v0.1.9) (2026-05-22)

### Features

* **loadbalancer:** reconcile `LoadBalancer` resources: create, sync and delete Scaleway Load Balancers ([#33](https://github.com/mathieubodin/scaleway-operator/issues/33))

## [0.1.8](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.7...scaleway-operator-v0.1.8) (2026-05-14)

### Features

* **metrics:** add the `scaleway_operator_instances_total` gauge
* **resilience:** stop calling the Scaleway API for a while after repeated failures (circuit breaker)

### Bug Fixes

* **reconciler:** back off exponentially on transient errors, from 30 seconds up to 5 minutes

## [0.1.7](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.6...scaleway-operator-v0.1.7) (2026-05-12)

No user-facing changes.

## [0.1.6](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.5...scaleway-operator-v0.1.6) (2026-05-11)

### Bug Fixes

* **server:** keep `/readyz` healthy when no resource is being reconciled

## [0.1.5](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.4...scaleway-operator-v0.1.5) (2026-05-11)

### Bug Fixes

* **server:** report ready right after startup instead of waiting for the first reconciliation

## [0.1.4](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.3...scaleway-operator-v0.1.4) (2026-05-11)

### Bug Fixes

* **image:** run as the numeric UID 65532, so Kubernetes can enforce `runAsNonRoot`

## [0.1.3](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.2...scaleway-operator-v0.1.3) (2026-05-11)

### Bug Fixes

* **image:** tag the image with the plain version (`0.1.3`) instead of the release tag, and publish its build attestation

## [0.1.2](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.1...scaleway-operator-v0.1.2) (2026-05-11)

### Features

* **server:** expose `/healthz`, `/readyz`, `/metrics` and `/log-level` on the operator HTTP server
* **metrics:** add Prometheus metrics for reconciliation errors and duration

## [0.1.1](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.0...scaleway-operator-v0.1.1) (2026-05-09)

### Features

* **image:** publish a multi-arch image (amd64, arm64) on GHCR, signed with cosign
* **charts:** publish the `scaleway-operator` and `scaleway-operator-crds` Helm charts as OCI artifacts on GHCR

### Bug Fixes

* **instance:** delete instances with the namespace credentials, accept an instance already gone, and keep internal URLs out of `status.error_message`

### Upgrade notes

* The API group is renamed from `scaleway.io` to `scaleway.mathieubodin.io`. Existing resources must be recreated under the new group.
