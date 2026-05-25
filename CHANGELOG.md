# Changelog

## [0.1.9](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.8...scaleway-operator-v0.1.9) (2026-05-22)

### Features

* **reconcilers:** add LoadBalancer reconciler with create/sync/delete lifecycle ([#33](https://github.com/mathieubodin/scaleway-operator/issues/33)) ([a6c78d2](https://github.com/mathieubodin/scaleway-operator/commit/a6c78d288b2f692b759c5d8eda75888de7a71434))

### Refactoring

* **reconcilers:** extract pure decision layer with decide_next_action ([#29](https://github.com/mathieubodin/scaleway-operator/issues/29)) ([e55ef90](https://github.com/mathieubodin/scaleway-operator/commit/e55ef908967d3291d287274db26f5775fcc15cfc))

### Documentation

* **claude:** update CLAUDE.md with missing make targets and integration test instructions ([0b4fcda](https://github.com/mathieubodin/scaleway-operator/commit/0b4fcda9c48546280553b7ab084ce99ef648253f))
* remove spurious blank lines in changelogs and docs ([15fa3ab](https://github.com/mathieubodin/scaleway-operator/commit/15fa3ab6589f2ae5b171e94574a48bf2bd5fdf80))
* **solutions:** clarify release-please extra-files doc is preventative guidance ([ccdfbc8](https://github.com/mathieubodin/scaleway-operator/commit/ccdfbc8f69352830149cb6887d3c23dbee2d7201))

## [0.1.8](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.7...scaleway-operator-v0.1.8) (2026-05-14)

### Features

* **chart:** add NOTES.txt to scaleway-operator-crds with orphaned CRD removal instructions ([b57fa8a](https://github.com/mathieubodin/scaleway-operator/commit/b57fa8afb7437f0519f2936864645bbbcd3f8f17))
* **metrics:** add instances_total gauge and circuit breaker state machine ([decf05f](https://github.com/mathieubodin/scaleway-operator/commit/decf05f9c78f504444b62dd60b03f5e033f096cf))
* **reconcilers:** wire gauge updates and circuit breaker ([aa57d64](https://github.com/mathieubodin/scaleway-operator/commit/aa57d64f2e79b85df119cf948f9c3597bd236d7a))

### Bug Fixes

* add exponential backoff on transient reconciliation errors ([d8b6c09](https://github.com/mathieubodin/scaleway-operator/commit/d8b6c0935a628e1cef376029d2952fbdac3c78d8))
* remove LoadBalancer and Project CRDs from Helm chart ([789a673](https://github.com/mathieubodin/scaleway-operator/commit/789a673cc425de85444d6ff840cecd2f9d14b55c))
* remove orphaned CRDs and complete README onboarding docs ([a1a1b31](https://github.com/mathieubodin/scaleway-operator/commit/a1a1b3133e26ff3e17bdbef6b23342b755c012a2))
* remove stale CRD YAMLs and correct ARCHITECTURE.md commands ([ff7065d](https://github.com/mathieubodin/scaleway-operator/commit/ff7065df65d50edbe89988dd2127ca09a8894a38))

### Documentation

* add ARCHITECTURE.md with extension guide ([f1ce625](https://github.com/mathieubodin/scaleway-operator/commit/f1ce6255c82bac84c4444813ebdb59da30bc709f))
* add STRATEGY.md ([84473ca](https://github.com/mathieubodin/scaleway-operator/commit/84473ca2fc09eff24562e513d86e9c3234c92b77))
* add STRATEGY.md ([09bef07](https://github.com/mathieubodin/scaleway-operator/commit/09bef07bf2906a3370ae7f4de51f3d9a38770e3f))
* **chart:** update scaleway-operator-crds README to remove orphaned CRDs ([e33be55](https://github.com/mathieubodin/scaleway-operator/commit/e33be555124ca4669a2654d8e0218b8d726081d5))
* remove manual run section from ARCHITECTURE.md ([9599743](https://github.com/mathieubodin/scaleway-operator/commit/9599743dde7690496332fbbe3184db9fa7bae27c))
* replace cargo run --example crd_gen with make generate-crds ([5509066](https://github.com/mathieubodin/scaleway-operator/commit/5509066a4780ac048f495b1814830f7f114bd538))
* **solutions:** add circuit breaker pattern and prometheus crate decision ([49a2f45](https://github.com/mathieubodin/scaleway-operator/commit/49a2f451a868fb0dba687791a4d7857f142f898e))
* **solutions:** add circuit breaker pattern and prometheus crate decision ([a793855](https://github.com/mathieubodin/scaleway-operator/commit/a7938555a72e8b3426203014474784a01c7572f3))
* update onboarding time target to &lt; 20 minutes ([1e3f47b](https://github.com/mathieubodin/scaleway-operator/commit/1e3f47bdf891c06888f10778661fec745bcefd46))

## [0.1.7](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.6...scaleway-operator-v0.1.7) (2026-05-12)

### Bug Fixes

* **charts:** align appVersion with deployed binary version 0.1.6 ([a7b7ccf](https://github.com/mathieubodin/scaleway-operator/commit/a7b7ccfdf1b4a190deb6ad203f868ac9a2d2e4c1))
* **release:** replace broken extra-files with post-release README sync job ([5c00738](https://github.com/mathieubodin/scaleway-operator/commit/5c0073825b947b523d1ca80a862036fcfe2c053b))
* **release:** replace broken extra-files with post-release README version sync ([0e4ff63](https://github.com/mathieubodin/scaleway-operator/commit/0e4ff63ce9dcb8b66e5b110e6d0f6198bc0e9b4e))
* **release:** use package-relative path for extra-files README ([b17dcc6](https://github.com/mathieubodin/scaleway-operator/commit/b17dcc6b7cfcbb45655afd72679ce2fd361c42d3))

### Documentation

* **readme:** improve installation section + fix charts appVersion ([12d21c4](https://github.com/mathieubodin/scaleway-operator/commit/12d21c43b700ca6b14676fd87398a8321093faf9))
* **readme:** improve installation section for new users ([67009b2](https://github.com/mathieubodin/scaleway-operator/commit/67009b2e4a20463e670f6fe8a79270af0965a053))
* **readme:** update installation with validated helm OCI commands ([05bc37a](https://github.com/mathieubodin/scaleway-operator/commit/05bc37a37290457fc26b1a50054b1e27e30f0476))
* **solutions:** add two bash and documentation editing conventions ([cd6387b](https://github.com/mathieubodin/scaleway-operator/commit/cd6387b6f8ea1b583b30009de5dbff896f5af09e))
* **solutions:** document release-please extra-files generic updater constraints ([a3833d5](https://github.com/mathieubodin/scaleway-operator/commit/a3833d58779ed3e307e1664845662c0c58e642a8))
* **solutions:** refresh 3 stale learnings + add 4 new installation debug docs ([d20c1d5](https://github.com/mathieubodin/scaleway-operator/commit/d20c1d5af55c680633e42eee162745d018750b2c))

## [0.1.6](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.5...scaleway-operator-v0.1.6) (2026-05-11)

### Bug Fixes

* **server:** add heartbeat ticker to keep /readyz alive with no instances ([5c3337c](https://github.com/mathieubodin/scaleway-operator/commit/5c3337c9970dba55db9469bc969a693dff34e74c))
* **server:** trigger rebuild for heartbeat fix ([72c6f70](https://github.com/mathieubodin/scaleway-operator/commit/72c6f70e941fa0393fc1b4f534a3640f93c67cda))

## [0.1.5](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.4...scaleway-operator-v0.1.5) (2026-05-11)

### Bug Fixes

* **server:** initialize last_reconcile_at to now at startup ([0f796e6](https://github.com/mathieubodin/scaleway-operator/commit/0f796e6b62eb1115099879788b67f5be42b21d5c))
* **server:** trigger rebuild for readyz startup fix ([bd8a65a](https://github.com/mathieubodin/scaleway-operator/commit/bd8a65aa4e2216b122472f5add870064ca42c67c))

## [0.1.4](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.3...scaleway-operator-v0.1.4) (2026-05-11)

### Bug Fixes

* **docker:** trigger rebuild with numeric UID fix ([3f1fc1c](https://github.com/mathieubodin/scaleway-operator/commit/3f1fc1c9682c5416541133a9894b549f802f4026))
* **docker:** use numeric UID 65532 for operator user ([5a5b894](https://github.com/mathieubodin/scaleway-operator/commit/5a5b89413e58f6119c36840c3713310578531405))

## [0.1.3](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.2...scaleway-operator-v0.1.3) (2026-05-11)

### Bug Fixes

* **ci:** add attestations: write permission to build-and-push job ([f75ee2b](https://github.com/mathieubodin/scaleway-operator/commit/f75ee2bfca2336a887f617df0d58d09aa6a69d9f))
* **ci:** correct Docker image tagging and attestation in release pipeline ([4667c72](https://github.com/mathieubodin/scaleway-operator/commit/4667c7226f1e940e18039b319d615008bc872847))
* **ci:** use version output instead of tag_name for Docker image tags ([73ea488](https://github.com/mathieubodin/scaleway-operator/commit/73ea488105a395ad22713962f8e1c6376937ab63))

## [0.1.2](https://github.com/mathieubodin/scaleway-operator/compare/scaleway-operator-v0.1.1...scaleway-operator-v0.1.2) (2026-05-11)

### Features

* **make:** add check-kubeconfig guard and run-integration-test-locally ([9538540](https://github.com/mathieubodin/scaleway-operator/commit/95385407b4392a301d43cf261cdc89338de703f0))
* **metrics:** add metrics module with ReconcileOutcome and OperatorMetrics ([ece2411](https://github.com/mathieubodin/scaleway-operator/commit/ece241116dd908234ccb740b8cd3c6c222ea49b3))
* **metrics:** extend Context with OperatorMetrics and last_reconcile_at ([649d39e](https://github.com/mathieubodin/scaleway-operator/commit/649d39e856e420e9aa66388321b3cb616a77c0b3))
* **metrics:** instrument reconcilers with error counter and ReconcileMeasurer RAII ([4d143fe](https://github.com/mathieubodin/scaleway-operator/commit/4d143feca16b1d0ae7811bf80ad3c637cc36abb6))
* **metrics:** prometheus observability and axum health server ([ab714db](https://github.com/mathieubodin/scaleway-operator/commit/ab714db615b0883c4d138772a1b899341f2525d6))
* **metrics:** replace health server with axum — /healthz, /readyz, /metrics, /log-level ([3461b2b](https://github.com/mathieubodin/scaleway-operator/commit/3461b2b1d812eca4938b10ceee8a68f263d3d33d))

### Bug Fixes

* **ci:** pin cosign binary to v2.5.2 for cosign-installer v4 upgrade ([0d051cf](https://github.com/mathieubodin/scaleway-operator/commit/0d051cf4ff4de3c0241ed857c8e809def2307607))
* **make:** add --namespace scaleway-system to deploy-crds ([13bbaad](https://github.com/mathieubodin/scaleway-operator/commit/13bbaadc04607127228f502f0ec560b3cea67290))
* **make:** deploy-crds via helm template | kubectl apply ([da1956b](https://github.com/mathieubodin/scaleway-operator/commit/da1956b19c96be486827e1ed1ca1d4671f8bd547))
* **metrics:** add as_str() to ReconcileOutcome, update CLAUDE.md modules ([e25ad7d](https://github.com/mathieubodin/scaleway-operator/commit/e25ad7dc1dfd2aef702067a241df1397e93a3085))
* **metrics:** address 4 code review findings ([3409c9e](https://github.com/mathieubodin/scaleway-operator/commit/3409c9ed7c21202e4ea09694000e78291d641c9e))
* **release:** correct changelog-path in release-please-config for helm packages ([b34195e](https://github.com/mathieubodin/scaleway-operator/commit/b34195e0103f5a383a8442c8f1536d58ae47a483))

### Refactoring

* **metrics:** simplify and fix code quality issues ([fe6beff](https://github.com/mathieubodin/scaleway-operator/commit/fe6beff2c487d6fbf1e249c789122c76321b54ad))

### Documentation

* **deploy:** document required RBAC for helm deploy and add manifest ([67bf6c2](https://github.com/mathieubodin/scaleway-operator/commit/67bf6c2c741b159e2ebba2958c78dae981747524))

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
