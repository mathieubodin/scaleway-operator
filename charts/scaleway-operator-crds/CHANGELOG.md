# Changelog

User-facing changes to the `scaleway-operator-crds` chart. The chart is versioned with the operator: a version without an entry below only follows an operator release.

## [0.1.14](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-crds-v0.1.13...scaleway-operator-crds-v0.1.14) (2026-10-10)

### Features

* **scaleway-secret:** add `status.observed_generation` to the `ScalewaySecret` CRD ([#127](https://github.com/mathieubodin/scaleway-operator/issues/127))

## [0.1.13](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-crds-v0.1.12...scaleway-operator-crds-v0.1.13) (2026-10-09)

### Features

* add the `ScalewaySecret` CRD ([#64](https://github.com/mathieubodin/scaleway-operator/issues/64))

## [0.1.12](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-crds-v0.1.11...scaleway-operator-crds-v0.1.12) (2026-06-12)

No user-facing changes.

## [0.1.11](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-crds-v0.1.10...scaleway-operator-crds-v0.1.11) (2026-06-03)

### Bug Fixes

* restore the `LoadBalancer` CRD, reconciled by the operator since 0.1.9

## [0.1.10](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-crds-v0.1.9...scaleway-operator-crds-v0.1.10) (2026-05-29)

No user-facing changes.

## [0.1.9](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-crds-v0.1.8...scaleway-operator-crds-v0.1.9) (2026-05-22)

No user-facing changes.

## [0.1.8](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-crds-v0.1.7...scaleway-operator-crds-v0.1.8) (2026-05-14)

### Upgrade notes

* The `LoadBalancer` and `Project` CRDs are removed from the chart: no reconciler handled them yet. CRDs carry `helm.sh/resource-policy: keep`, so they stay in the cluster after the upgrade. `NOTES.txt` gives the commands to delete them.

## [0.1.7](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-crds-v0.1.6...scaleway-operator-crds-v0.1.7) (2026-05-12)

No user-facing changes.

## [0.1.6](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-crds-v0.1.5...scaleway-operator-crds-v0.1.6) (2026-05-11)

No user-facing changes.

## [0.1.5](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-crds-v0.1.4...scaleway-operator-crds-v0.1.5) (2026-05-11)

No user-facing changes.

## [0.1.4](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-crds-v0.1.3...scaleway-operator-crds-v0.1.4) (2026-05-11)

No user-facing changes.

## [0.1.3](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-crds-v0.1.2...scaleway-operator-crds-v0.1.3) (2026-05-11)

No user-facing changes.

## [0.1.2](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-crds-v0.1.1...scaleway-operator-crds-v0.1.2) (2026-05-11)

No user-facing changes.
