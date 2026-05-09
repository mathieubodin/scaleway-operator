# Changelog

## [0.1.1](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.0...scaleway-operator-v0.1.1) (2026-05-09)


### Features

* **ci:** add GitHub Actions release workflow with cosign signing ([464c5f5](https://github.com/mathieubodin/scaleway-operator/commit/464c5f5671c0db3e0fe8c263bb31808b9b83c01e))
* **ci:** publish Helm charts as OCI artifacts to GHCR ([b005b5b](https://github.com/mathieubodin/scaleway-operator/commit/b005b5b8a36d904a4313fcc217f9d10e090060f9))
* **context:** add scaleway_base_url field to Context for testable namespace client injection ([8d6f3d3](https://github.com/mathieubodin/scaleway-operator/commit/8d6f3d3024eba5d30b0c4987bec00e70b98bb750))
* **docker:** rewrite Dockerfile with cargo-chef and cargo-zigbuild ([e29472b](https://github.com/mathieubodin/scaleway-operator/commit/e29472b3d6009c9fa6818462c9c2430e6ffe98f8))
* **helm:** add scaleway-operator chart ([2b75c7a](https://github.com/mathieubodin/scaleway-operator/commit/2b75c7a0d04e9426c1ca22d37874b46d96937483))
* **helm:** add scaleway-operator-crds chart ([e6f0f14](https://github.com/mathieubodin/scaleway-operator/commit/e6f0f14cb85a54d9889845946f35fd47ac3122de))
* **k8s:** add crd-gen example and generate CRD manifests ([ea53033](https://github.com/mathieubodin/scaleway-operator/commit/ea530337d4fcef56ea04a6c240baff2ebcf15b80))
* **k8s:** add deployment manifest with RBAC and secretKeyRef injection ([8cba1d1](https://github.com/mathieubodin/scaleway-operator/commit/8cba1d1a97f61b730b8dc3970cecf9b532c0bb8e))
* **registry:** update REGISTRY to GHCR and deployment image ref ([69102a5](https://github.com/mathieubodin/scaleway-operator/commit/69102a58fcbd4eb4a224047cae87d1382837648d))
* release pipeline, Helm charts, domain fix, and CI hardening ([4403d6f](https://github.com/mathieubodin/scaleway-operator/commit/4403d6f3f746543758176b42fa49eab2be3405c5))
* **tests:** add integration test infrastructure with TestFixture, k8s helpers, and all test scenarios ([a7528af](https://github.com/mathieubodin/scaleway-operator/commit/a7528aff2954665c75dd7159833b561f729d586a))


### Bug Fixes

* **ci:** per-job permissions, pin actions to SHA, fix helm login and packaging ([d3e740d](https://github.com/mathieubodin/scaleway-operator/commit/d3e740d67b3f3f2ffb33043e6e004644c0887dc3))
* **domain:** rename scaleway.io → scaleway.mathieubodin.io across codebase ([6cf0d6a](https://github.com/mathieubodin/scaleway-operator/commit/6cf0d6ac914d668d12040f6cc47c2d58c1f9f5d4))
* **lint:** resolve dead_code and clippy warnings to pass make check ([fe72315](https://github.com/mathieubodin/scaleway-operator/commit/fe7231569fab6271e94621af10e594e6c728fa55))
* **make:** deterministic chart selection, remove --force, KUBECONFIG variable ([2cfe907](https://github.com/mathieubodin/scaleway-operator/commit/2cfe9074cfc6e03172d59cd9e72c4e8c1928fbfa))
* **reconciler:** use ns_client in handle_deletion, handle InstanceNotFound, sanitize status errors ([c478ab2](https://github.com/mathieubodin/scaleway-operator/commit/c478ab20cba9692129adcf62960cba9261cd0350))
* rename CRD label key and refactor check-helm to tool-guard only ([ac5f35f](https://github.com/mathieubodin/scaleway-operator/commit/ac5f35fd1c9bb04c8b99ccca0f61ddda39855f93))
* **review:** address code review findings — safety, tests, reliability ([b9581e7](https://github.com/mathieubodin/scaleway-operator/commit/b9581e7ebb89a7f9cef5133548419ec3be7d9f21))
* **review:** address remaining 3 deferred findings ([98e9c8c](https://github.com/mathieubodin/scaleway-operator/commit/98e9c8cc77444cc2a7d9008e74fab4d945f6a4dc))
* separate changelogs, TARGETPLATFORM default, Helm schema validation ([bae58b7](https://github.com/mathieubodin/scaleway-operator/commit/bae58b76240b8c13fe664ec74b722b9deef04c31))
* **tests:** support KUBE_API_URL env var for kubectl proxy (default: localhost:8001) ([763328a](https://github.com/mathieubodin/scaleway-operator/commit/763328a895ae55dda9026817a3a19807f12cfe90))
* **tests:** use pre-created fixtures via YAML, no k8s resource creation from test code ([7d61dfa](https://github.com/mathieubodin/scaleway-operator/commit/7d61dfac27a7811e5e946192f527196e03c91dcb))


### Refactoring

* **lib:** export modules from lib.rs for integration test access ([efb83ac](https://github.com/mathieubodin/scaleway-operator/commit/efb83ac052b130e0bf604d79764db0b744b2ed6e))


### Documentation

* mark k8s manifests plan as completed ([84a0663](https://github.com/mathieubodin/scaleway-operator/commit/84a0663ad5a98a34eda3e8f652508e672a0ab2cc))
* **readme:** add end-to-end deployment tutorial with lifecycle state diagram ([d58b8d5](https://github.com/mathieubodin/scaleway-operator/commit/d58b8d53a8b99e1a6e013aa24a6dbebd776b1f81))
* **solutions:** add Makefile guard pattern convention and extend domain doc ([babbe37](https://github.com/mathieubodin/scaleway-operator/commit/babbe37e59786ffe7473cba85d1ce78b4196f6e6))
* **solutions:** document Kubernetes CRD API group domain ownership convention ([5a7f980](https://github.com/mathieubodin/scaleway-operator/commit/5a7f980e61142d3d88095293f63e523a1eabb737))
