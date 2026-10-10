# Changelog

User-facing changes to the `scaleway-operator` chart. The chart is versioned with the operator: a version without an entry below only follows an operator release.

## [0.1.13](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-chart-v0.1.12...scaleway-operator-chart-v0.1.13) (2026-10-09)

### Features

* **rbac:** grant the operator access to `scalewaysecrets` and their status
* **namespace-bootstrap:** create, in each bootstrapped namespace, a Role and RoleBinding that let the operator `get` Secrets ([#64](https://github.com/mathieubodin/scaleway-operator/issues/64))

### Upgrade notes

* The ClusterRole no longer grants any access to Secrets. `ScalewaySecret` only works in namespaces listed in `namespaceBootstrap`.
* Starting with this version, releases of this chart are tagged `scaleway-operator-chart-v*`. Earlier versions shared the `scaleway-operator-v*` tags of the operator.

## [0.1.12](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.11...scaleway-operator-v0.1.12) (2026-06-12)

No user-facing changes.

## [0.1.11](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.10...scaleway-operator-v0.1.11) (2026-06-12)

No user-facing changes.

## [0.1.10](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.9...scaleway-operator-v0.1.10) (2026-06-03)

No user-facing changes.

## [0.1.9](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.8...scaleway-operator-v0.1.9) (2026-05-29)

No user-facing changes.

## [0.1.8](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.7...scaleway-operator-v0.1.8) (2026-05-22)

No user-facing changes.

## [0.1.7](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.6...scaleway-operator-v0.1.7) (2026-05-12)

### Bug Fixes

* align `appVersion` with the released operator version

## [0.1.6](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.5...scaleway-operator-v0.1.6) (2026-05-11)

No user-facing changes.

## [0.1.5](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.4...scaleway-operator-v0.1.5) (2026-05-11)

No user-facing changes.

## [0.1.4](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.3...scaleway-operator-v0.1.4) (2026-05-11)

No user-facing changes.

## [0.1.3](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.2...scaleway-operator-v0.1.3) (2026-05-11)

No user-facing changes.

## [0.1.2](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.1...scaleway-operator-v0.1.2) (2026-05-11)

### Bug Fixes

* **deployment:** point the liveness and readiness probes at `/healthz` and `/readyz`
