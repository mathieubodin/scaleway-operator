# Changelog

User-facing changes to the operator binary and its container image. Chart changes are in `charts/*/CHANGELOG.md`.

## [0.1.15](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.14...scaleway-operator-v0.1.15) (2026-10-10)


### Features

* **ops:** copy milestone, labels and project from the linked issue to pull requests ([d37e211](https://github.com/mathieubodin/scaleway-operator/commit/d37e2117ed76fc20eae7deec00bac82432ed04a2))
* **ops:** copy milestone, labels and project from the linked issue to pull requests ([ae66d32](https://github.com/mathieubodin/scaleway-operator/commit/ae66d32aa1b61e0638af6be136d91e09ddf7ac1b)), refs [#152](https://github.com/mathieubodin/scaleway-operator/issues/152)
* **ops:** fall back to the issue cited by Refs when a pull request closes none ([3b1161f](https://github.com/mathieubodin/scaleway-operator/commit/3b1161ff42f0e9c0f481d367b55ca2d31b7429e7))
* **ops:** record produced tokens on the pull request project item ([db62180](https://github.com/mathieubodin/scaleway-operator/commit/db621809a67b0f6628c1fe558374bf88b8e93138))
* **ops:** report produced tokens per pull request from a script and a hook ([18e810e](https://github.com/mathieubodin/scaleway-operator/commit/18e810e3a1fce659f7f63891005488a2627ab9b2))
* **ops:** report produced tokens per pull request from a script and a hook ([2be70e9](https://github.com/mathieubodin/scaleway-operator/commit/2be70e9594b508c132e3b8a7cdfea2f4e0c46feb)), refs [#149](https://github.com/mathieubodin/scaleway-operator/issues/149)


### Bug Fixes

* **scaleway-secret:** let Scaleway disable the previous version on each push ([2a6cf34](https://github.com/mathieubodin/scaleway-operator/commit/2a6cf34d969fb8381574952398401d92bd56f9ac))
* **scaleway-secret:** let Scaleway disable the previous version on each push ([a5c86a4](https://github.com/mathieubodin/scaleway-operator/commit/a5c86a40338b2566d42860960ed9057710c02f70)), refs [#117](https://github.com/mathieubodin/scaleway-operator/issues/117)
* **scaleway-secret:** recheck source-side errors and retry transient credential lookups ([238b115](https://github.com/mathieubodin/scaleway-operator/commit/238b1158028c037592d55640a5742d15a7bea480))
* **scaleway-secret:** recheck source-side errors and retry transient credential lookups ([19772d0](https://github.com/mathieubodin/scaleway-operator/commit/19772d06d98f3887a7e82bcd649f9ce89820757d)), refs [#128](https://github.com/mathieubodin/scaleway-operator/issues/128)
* **scaleway-secret:** resync when the CR spec changes ([f149e2e](https://github.com/mathieubodin/scaleway-operator/commit/f149e2e388951a74c019ed414b9a1cb6bc4ea336))
* **scaleway-secret:** resync when the CR spec changes ([d4d899e](https://github.com/mathieubodin/scaleway-operator/commit/d4d899e0620f4c3da3b9ebcad45610ad2e379e63)), refs [#127](https://github.com/mathieubodin/scaleway-operator/issues/127)

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
