# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

Use `make` as the single entry point (see `Makefile` for full list, `make help` to display):

- Tester la conformite de l'environnement : `make env-check`
- Tester (unitaires, rapide)              : `make coverage-text`
- Tester l'application (rapport HTML)     : `make coverage`
- Tester l'application (rapport JSON/CI)  : `make coverage-json`
- Générer les manifests CRD               : `make generate-crds`
- Construire le binaire                   : `make build`
- Lint et format                          : `make check`
- Nettoyer les artefacts                  : `make clean`
- Construire l'image                      : `make image-build`
- Distribuer l'image                      : `make image-push`
- Deployer les CRDS                       : `make deploy-crds`
- Deployer la stack operateur             : `make deploy`
- Verifier l'etat du deploiement          : `make deploy-status`

> ⚠️ Toute modification de `src/resources.rs` (CRDs) doit être suivie de `make generate-crds`
> pour régénérer les manifests dans `k8s/`.

**Tests d'intégration via kind (cluster éphémère) :**

```bash
make test-integration-kind  # crée un cluster kind, joue les 12 tests, supprime le cluster
```

Le cluster kind `scaleway-operator-test` est créé et supprimé automatiquement. Le fichier
`.kube/kind-config` est créé temporairement et nettoyé à la fin, même en cas d'échec.

**Prérequis pour `make test-integration-kind` :**

- Docker : disponible et démarré
- kind : `brew install kind` (macOS) ou `go install sigs.k8s.io/kind@latest` (Linux)
- helm : `brew install helm`

**Prérequis pour les targets deploy :**

- Kubeconfig : préférer `KUBECONFIG=~/.kube/config make deploy-crds` (standard).
  `.kube/config` à la racine du repo fonctionne aussi mais ne doit jamais être commité
  (credentials cluster — déjà dans `.gitignore`).
- Credentials Scaleway pour `make deploy` : passer via `--values` ou
  `HELM_EXTRA_FLAGS="--set scaleway.token=<token> --set scaleway.organizationId=<uuid>"`.
- Pour forcer une mise à jour helm (cas de récupération) :
  `HELM_EXTRA_FLAGS=--force make deploy-crds`

**RBAC requis sur le cluster (à appliquer une fois par cluster) :**

`helm upgrade --install` stocke son état de release comme des Secrets dans le namespace
`scaleway-system`, et les CRDs sont des ressources cluster-scoped. L'utilisateur Kubernetes
doit avoir les droits suivants :

| Scope | Ressource | Verbes |
| --- | --- | --- |
| Cluster | `apiextensions.k8s.io/customresourcedefinitions` | get, list, create, update, patch, delete |
| Namespace `scaleway-system` | `secrets`, `configmaps` | get, list, watch, create, update, patch, delete |

Sur Scaleway Kapsule, le nom d'utilisateur est de la forme
`scaleway:bearer:<uuid-du-token-iam>`.

## Architecture

Opérateur Kubernetes écrit en Rust avec [kube-rs](https://kube.rs/). Il réconcilie des Custom Resources Scaleway avec l'API Scaleway.

### Modules

- **`main.rs`**:
      - Initialise le tracing
      - Lit les variables d'environnement (`SCALEWAY_TOKEN`, `SCALEWAY_ORG_ID`)
      - Construit le `Context` partagé
      - Lance le `Controller` kube-rs sur la ressource `Instance`.
- **`resources.rs`**: Définit les CRDs via la macro `#[derive(CustomResource)]` : `Instance`, `Project`, `LoadBalancer`, `NamespaceRole` (cluster-wide).
- **`context.rs`**: Struct `Context` partagé entre les réconciliateurs.
    Contient aussi les helpers pour extraire les annotations de namespace (`scaleway.mathieubodin.io/project-id`, `scaleway.mathieubodin.io/organization-id`) et `get_scaleway_role_for_namespace` qui cherche la ressource `NamespaceRole` par nom de namespace.

- **`reconcilers.rs`** — `reconcile_instance` : logique de réconciliation en 9 étapes (rôle namespace → project_id → finalizer → validation → create/sync). `error_policy` requeue après 60s en cas d'erreur.
    La décision est séparée dans `decide_next_action(&input) -> ReconcileDecision` (couche pure, testable unitairement) ; les effets de bord sont dans `reconcile_instance_inner`.
- **`scaleway.rs`** — `ScalewayClient` wrappant `reqwest`. Appels REST à `https://api.scaleway.com`. Authentification via header `X-Auth-Token`.
- **`error.rs`** — `OperatorError` enum avec `thiserror`, couvrant les erreurs kube, Scaleway, réseau et configuration. Expose `metric_label()` pour produire le label Prometheus PascalCase de chaque variant.
- **`metrics.rs`** — `ReconcileOutcome` enum et `OperatorMetrics` struct (compteur `scaleway_operator_reconcile_errors_total` + histogramme `scaleway_operator_reconcile_duration_seconds`). `ReconcileMeasurer` RAII dans `reconcilers.rs` consomme ces handles.
- **`server.rs`** — Serveur axum sur `:8080` exposant `/healthz` (liveness), `/readyz` (readiness — vérifie `last_reconcile_at` dans les 60 dernières secondes), `/metrics` (Prometheus text), `/log-level` (lecture seule).

### Flux de réconciliation (Instance)

1. Récupère le `NamespaceRole` cluster-wide dont le nom correspond au namespace de l'instance (erreur bloquante si absent).
2. Lit l'annotation `scaleway.mathieubodin.io/project-id` sur le namespace (erreur bloquante si absente).
3. Vérifie le `deletion_timestamp` pour la suppression (appel DELETE Scaleway + retrait du finalizer).
4. Ajoute le finalizer `scaleway.mathieubodin.io/instance-finalizer` si absent, puis requeue.
5. Valide la zone et le type d'instance (listes statiques dans `scaleway.rs`).
6. Vérifie l'accès au projet via `GET /account/v3/projects/{id}`.
7. Crée l'instance Scaleway si `status.scaleway_id` est absent.
8. Synchronise l'état depuis Scaleway et met à jour le `status`.
9. Requeue toutes les 30 secondes pour la synchronisation périodique.

### Variables d'environnement requises

| Variable          | Obligatoire | Description                                                              |
|-------------------|-------------|--------------------------------------------------------------------------|
| `SCALEWAY_TOKEN`  | Oui         | Token API Scaleway (nécessite `InstancesFullAccess` + `ProjectReadOnly`) |
| `SCALEWAY_ORG_ID` | Oui         | ID de l'organisation                                                     |

### Prérequis namespace

Chaque namespace hébergeant des `Instance` doit avoir :

- L'annotation `scaleway.mathieubodin.io/project-id` sur le namespace
- Une ressource `NamespaceRole` cluster-wide dont le `.metadata.name` correspond exactement au nom du namespace

### CRDs déployées

- `instances.scaleway.mathieubodin.io` (namespaced)
- `projects.scaleway.mathieubodin.io` (namespaced)
- `namespaceroles.scaleway.mathieubodin.io` (cluster-wide)

### Documentation

`docs/solutions/` — solutions documentées à des problèmes passés (patterns architecturaux, bugs, conventions), organisées par catégorie avec frontmatter YAML (`module`, `tags`, `problem_type`). Utile lors de l'implémentation ou du débogage dans des zones déjà documentées.
