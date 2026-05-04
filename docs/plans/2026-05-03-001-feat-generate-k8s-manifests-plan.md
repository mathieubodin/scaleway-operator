---
title: "feat: Générer les manifests Kubernetes manquants dans k8s/"
type: feat
status: completed
date: 2026-05-03
---

# feat: Générer les manifests Kubernetes manquants dans k8s/

## Overview

Le répertoire `k8s/` est vide alors que le README et le Makefile référencent trois fichiers essentiels (`crd-instance.yaml`, `crd-namespacerole.yaml`, `deployment.yaml`). Sans ces fichiers, l'opérateur ne peut pas être déployé. Ce plan crée ces manifests ainsi qu'un binaire de génération CRD pour assurer la maintenabilité à long terme.

---

## Problem Frame

L'opérateur Kubernetes est fonctionnel côté code (47 tests, compilation propre) mais ne peut pas être installé : `make deploy-crd` et `make deploy` échouent immédiatement car les fichiers YAML cibles n'existent pas. Le README documente les prérequis corrects (NamespaceRole, annotation, Secret) mais sans les manifests correspondants, un utilisateur ne peut pas les appliquer.

---

## Requirements Trace

- R1. Les CRDs `instances.scaleway.io`, `namespaceroles.scaleway.io`, `projects.scaleway.io` doivent pouvoir être appliquées via `kubectl apply -f k8s/`
- R2. Le `deployment.yaml` doit injecter `SCALEWAY_TOKEN` et `SCALEWAY_ORG_ID` depuis un Secret via `secretKeyRef`
- R3. Le RBAC doit autoriser l'opérateur à lire les NamespaceRole (cluster-wide), les namespaces, les secrets dans `scaleway-system`, et réconcilier les instances
- R4. Les manifests CRD doivent rester synchronisés avec les structs Rust (`InstanceSpec`, `NamespaceRoleSpec`, etc.)

---

## Scope Boundaries

- Les CRDs `loadbalancers.scaleway.io` et `projects.scaleway.io` sont définies dans `resources.rs` mais non réconciliées — elles peuvent être générées mais ne sont pas requises pour le déploiement opérationnel
- Ce plan ne déploie pas l'opérateur sur un cluster réel
- Ce plan ne configure pas de pipeline CI pour la génération automatique des CRDs

---

## Context & Research

### Relevant Code and Patterns

- `src/resources.rs` — Définit `InstanceSpec`, `NamespaceRoleSpec`, `ProjectSpec`, `LoadBalancerSpec` via `#[derive(CustomResource)]`
- `Cargo.toml` — `kube = { version = "3.1.0", features = ["runtime", "derive", "client"] }` ; `k8s-openapi = "0.27.1"` ; pas de `serde_yaml` actuellement
- `Makefile` — `deploy-crd` référence seulement `crd-instance.yaml` et `crd-project.yaml` ; `deploy` fait `kubectl apply -f k8s/deployment.yaml`
- `src/reconcilers.rs` — lit des Secrets dans `scaleway-system`, lit des NamespaceRole cluster-wide, réconcilie des Instance namespaced

### Institutional Learnings

- `docs/solutions/architecture-patterns/namespacerole-namespace-annotation-scaleway-multiproject-2026-05-03.md` — NamespaceRole est cluster-wide (pas de `#[kube(namespaced)]`), lookup par nom = namespace

---

## Key Technical Decisions

- **Génération vs écriture manuelle des CRDs** : kube-rs expose `kube::core::CustomResourceExt::crd()` qui génère l'objet `CustomResourceDefinition` directement depuis les macros Rust. Un binaire `src/bin/crd-gen.rs` est préférable à l'écriture manuelle car il reste synchronisé avec le code. Si le schéma change dans `resources.rs`, `make generate-crds` régénère les YAML. L'alternative (écriture manuelle) crée une divergence silencieuse.

- **Sérialisation YAML** : `serde_yaml` n'est pas dans les dépendances actuelles. Deux options : (a) ajouter `serde_yaml` en `[dev-dependencies]` — uniquement utilisé par le binaire de génération, zéro impact sur le binaire de production ; (b) sérialiser en JSON (valide YAML) via `serde_json`. L'option (a) produit des YAML lisibles ; choisir (a).

- **Namespace opérateur** : `scaleway-system` — déjà utilisé dans `reconcilers.rs` (`NAMESPACE_CREDS_NS`) et le README.

- **Image dans deployment.yaml** : Utiliser un placeholder `${REGISTRY}/${IMAGE_NAME}:${IMAGE_TAG}` ou une valeur par défaut `scaleway-operator:latest` qui correspond aux variables Makefile. Ne pas hardcoder de registre.

- **RBAC minimal** : L'opérateur a besoin de :
  - `instances.scaleway.io` — verbs: get, list, watch, update, patch (namespaced)
  - `namespaceroles.scaleway.io` — verbs: get, list, watch (cluster-wide)
  - `namespaces` (core) — verbs: get (pour lire les annotations)
  - `secrets` dans `scaleway-system` — verbs: get (pour lire `scaleway-ns-creds-*`)
  - `events` (core) — verbs: create, patch (pour les events Kubernetes)

---

## Open Questions

### Resolved During Planning

- **`serde_yaml` en dev-dep ou dep ?** : Dev-dep uniquement — le binaire `crd-gen` n'est jamais inclus dans le binaire de production.
- **Faut-il inclure les CRDs `LoadBalancer` et `Project` ?** : Oui, les générer mais les marquer comme "non réconciliées" dans un commentaire. Elles peuvent être appliquées sans risque.
- **Format d'image dans deployment.yaml ?** : `scaleway-operator:latest` par défaut, avec un commentaire invitant à le remplacer par le chemin de registre réel.

### Deferred to Implementation

- Le schéma OpenAPIv3 exact généré par kube-rs devra être vérifié après génération (especially les optional fields avec `#[serde(default)]`).
- La configuration de `imagePullPolicy` (`Always` vs `IfNotPresent`) dépend du contexte de déploiement — laisser `IfNotPresent` comme défaut raisonnable.

---

## Output Structure

    k8s/
    ├── crd-instance.yaml         # CRD instances.scaleway.io (namespaced)
    ├── crd-project.yaml          # CRD projects.scaleway.io (namespaced)
    ├── crd-namespacerole.yaml    # CRD namespaceroles.scaleway.io (cluster-wide)
    ├── crd-loadbalancer.yaml     # CRD loadbalancers.scaleway.io (namespaced, non réconciliée)
    ├── deployment.yaml           # Namespace + ServiceAccount + RBAC + Deployment
    └── examples.yaml             # Exemples namespace + NamespaceRole + Instance

    src/
    └── bin/
        └── crd-gen.rs            # Binaire de génération CRD

---

## Implementation Units

- U1. **Binaire `crd-gen` et target Makefile**

**Goal:** Ajouter un binaire Rust `src/bin/crd-gen.rs` qui génère les CRDs de l'opérateur en YAML en utilisant `CustomResourceExt::crd()`, et un target `make generate-crds` dans le Makefile.

**Requirements:** R4

**Dependencies:** Aucune

**Files:**
- Create: `src/bin/crd-gen.rs`
- Modify: `Cargo.toml` (ajouter `serde_yaml` en `[dev-dependencies]`)
- Modify: `Makefile` (ajouter target `generate-crds`)

**Approach:**
- Importer `Instance`, `NamespaceRole`, `Project`, `LoadBalancer` depuis `crate::resources`
- Appeler `<Type as kube::core::CustomResourceExt>::crd()` pour chaque type
- Sérialiser avec `serde_yaml::to_string()` séparé par `---`
- Écrire chaque CRD dans `k8s/crd-{resource}.yaml` via `std::fs::write`
- Target Makefile : `generate-crds: check-cargo ## Génère les CRDs depuis le code Rust`

**Patterns to follow:**
- `src/resources.rs` pour les imports de types
- Pattern `[[bin]]` dans `Cargo.toml` n'est pas nécessaire pour les binaires dans `src/bin/`

**Test scenarios:**
- Test expectation: none — binaire utilitaire de génération, pas de logique métier

**Verification:**
- `cargo run --bin crd-gen` s'exécute sans erreur
- Les fichiers `k8s/crd-*.yaml` sont créés avec des YAML valides contenant `kind: CustomResourceDefinition`
- Chaque CRD contient le bon `spec.group: scaleway.io` et `spec.names.kind`

---

- U2. **Manifests CRD dans `k8s/`**

**Goal:** Générer et committer les 4 fichiers CRD YAML en exécutant le binaire `crd-gen` et vérifier leur contenu.

**Requirements:** R1

**Dependencies:** U1

**Files:**
- Create: `k8s/crd-instance.yaml`
- Create: `k8s/crd-namespacerole.yaml`
- Create: `k8s/crd-project.yaml`
- Create: `k8s/crd-loadbalancer.yaml`

**Approach:**
- Exécuter `cargo run --bin crd-gen` (ou `make generate-crds`)
- Vérifier que chaque fichier contient les bonnes colonnes print, le bon scope (`Namespaced` vs `Cluster`), et les bons champs spec
- `crd-namespacerole.yaml` doit avoir `spec.scope: Cluster`
- Ajouter un commentaire en tête de `crd-loadbalancer.yaml` : `# Note: CRD définie mais non réconciliée par l'opérateur (v0.1)`

**Patterns to follow:**
- Vérification : `kubectl apply --dry-run=client -f k8s/crd-namespacerole.yaml`

**Test scenarios:**
- Test expectation: none — fichiers générés, pas de logique testable en unitaire

**Verification:**
- `kubectl apply --dry-run=client -f k8s/crd-instance.yaml` retourne `created (dry run)`
- `kubectl apply --dry-run=client -f k8s/crd-namespacerole.yaml` retourne `created (dry run)`
- `grep 'scope: Cluster' k8s/crd-namespacerole.yaml` retourne un résultat

---

- U3. **`k8s/deployment.yaml`**

**Goal:** Créer le manifest de déploiement complet de l'opérateur : Namespace, ServiceAccount, ClusterRole, ClusterRoleBinding, Role, RoleBinding, et Deployment.

**Requirements:** R2, R3

**Dependencies:** Aucune (peut être fait en parallèle de U1/U2)

**Files:**
- Create: `k8s/deployment.yaml`

**Approach:**
- **Namespace** : `scaleway-system`
- **ServiceAccount** : `scaleway-operator` dans `scaleway-system`
- **ClusterRole** : permissions sur les CRDs cluster-wide et namespaced, namespaces (core), et instances
  - `namespaceroles.scaleway.io` — `get`, `list`, `watch`
  - `instances.scaleway.io` — `get`, `list`, `watch`, `update`, `patch`
  - `namespaces` (core) — `get`
- **ClusterRoleBinding** : lie le ClusterRole au ServiceAccount
- **Role** dans `scaleway-system` : `secrets` — `get`, `list` (pour lire `scaleway-ns-creds-*`)
- **RoleBinding** dans `scaleway-system` : lie le Role au ServiceAccount
- **Deployment** :
  - Image : `scaleway-operator:latest` (commentaire : "remplacer par votre registre")
  - `imagePullPolicy: IfNotPresent`
  - Variables d'environnement via `secretKeyRef` sur le secret `scaleway-credentials` :
    - `SCALEWAY_TOKEN` ← `scaleway-credentials.SCALEWAY_TOKEN`
    - `SCALEWAY_ORG_ID` ← `scaleway-credentials.SCALEWAY_ORG_ID`
  - Port `8080` pour le health check
  - `livenessProbe` et `readinessProbe` sur `GET /` port 8080
  - `resources.requests` : `cpu: 50m`, `memory: 64Mi`
  - `resources.limits` : `memory: 256Mi`

**Test scenarios:**
- Test expectation: none — manifest de configuration, pas de logique testable en unitaire

**Verification:**
- `kubectl apply --dry-run=client -f k8s/deployment.yaml` retourne `created (dry run)` pour chaque ressource
- Le Deployment référence bien le ServiceAccount et monte les env vars depuis le secret

---

- U4. **`k8s/examples.yaml` et mise à jour du Makefile**

**Goal:** Créer des exemples complets illustrant le workflow NamespaceRole + annotation + Instance, et mettre à jour le Makefile pour inclure `crd-namespacerole.yaml` dans `deploy-crd`.

**Requirements:** R1

**Dependencies:** U2, U3

**Files:**
- Create: `k8s/examples.yaml`
- Modify: `Makefile`

**Approach:**

`examples.yaml` contient séparés par `---` :
1. Annotation du namespace `production` (`scaleway.io/project-id`)
2. `NamespaceRole` nommée `production` avec `scaleway_role: Editor`
3. `Instance` dans le namespace `production` sans `project_id` dans le spec
4. Exemple `NamespaceRole` read-only pour namespace `staging` avec `scaleway_role: Viewer`

Makefile :
- `deploy-crd` : ajouter `kubectl apply -f k8s/crd-namespacerole.yaml`
- Ajouter target `generate-crds: check-cargo ## Génère les manifests CRD depuis le code Rust`

**Test scenarios:**
- Test expectation: none — fichiers de configuration et exemples

**Verification:**
- `kubectl apply --dry-run=client -f k8s/examples.yaml` s'exécute sans erreur
- `make deploy-crd` applique maintenant les 3 CRDs (instance, project, namespacerole)

---

## System-Wide Impact

- **Interaction graph :** Le `deployment.yaml` crée un ClusterRoleBinding qui donne accès en lecture aux NamespaceRole cluster-wide et aux namespaces. Tout changement aux verbs RBAC dans `reconcilers.rs` (ex: écriture sur des CRDs) nécessitera de mettre à jour le ClusterRole.
- **Unchanged invariants :** Le code Rust (`src/`) n'est pas modifié par ce plan — uniquement des fichiers de configuration YAML sont ajoutés.
- **API surface parity :** Le `make deploy-crd` dans le Makefile doit référencer exactement les mêmes fichiers que ceux générés par `crd-gen` — garder les deux synchronisés.

---

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Le binaire `crd-gen` génère un schéma incompatible avec la version du cluster Kubernetes cible | Tester avec `kubectl apply --dry-run=client` et `kubectl diff` avant tout déploiement réel |
| Les champs optionnels (`#[serde(default)]`) peuvent être représentés différemment dans le JSON Schema généré vs attendu | Vérifier manuellement le schéma généré pour `InstanceSpec.network` et `InstanceSpec.security` (optionnels) |
| `serde_yaml` en `[dev-dependencies]` peut casser si une version incompatible avec `serde` est ajoutée plus tard | Utiliser `serde_yaml = { version = "0.9" }` (version stable actuelle) |

---

## Documentation / Operational Notes

- Après tout changement dans `src/resources.rs` (ajout de champ, nouveau type CRD), exécuter `make generate-crds` et committer les fichiers mis à jour
- Le `deployment.yaml` utilise une image `scaleway-operator:latest` — remplacer par l'image poussée dans le registre avant tout déploiement réel
- Le secret `scaleway-credentials` doit exister dans `scaleway-system` avant `make deploy`

---

## Sources & References

- Relevant code: `src/resources.rs`, `Makefile`, `src/reconcilers.rs`
- kube-rs CustomResourceExt: https://docs.rs/kube/latest/kube/core/trait.CustomResourceExt.html
