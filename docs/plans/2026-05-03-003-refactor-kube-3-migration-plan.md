---
title: "refactor: Migrate to kube 3.1.0 / k8s-openapi 0.27.1"
type: refactor
status: active
date: 2026-05-03
---

# refactor: Migrate to kube 3.1.0 / k8s-openapi 0.27.1

## Overview

`Cargo.toml` a été mis à jour vers `kube = "3.1.0"` et `k8s-openapi = { version = "0.27.1", features = ["v1_35"] }`. Cette mise à jour introduit trois breaking changes qui empêchent la compilation. Ce plan les corrige.

---

## Problem Frame

kube 3.x a rendu `JsonSchema` obligatoire sur les structs dérivant `CustomResource`, a invalidé la syntaxe `#[kube(namespaced = false)]`, et dépend de hyper 1.x — incompatible avec `reqwest 0.11`. Le code source n'a pas encore été adapté.

---

## Requirements Trace

- R1. Le projet compile avec `kube = "3.1.0"` et `k8s-openapi = "0.27.1"`.
- R2. Tous les tests unitaires existants continuent de passer.
- R3. Le comportement runtime des CRDs est préservé (NamespaceRole reste cluster-scoped).

---

## Scope Boundaries

- Ne pas modifier la logique de réconciliation ni les règles métier.
- Ne pas régénérer ni modifier les CRD YAMLs dans `k8s/` — ils restent inchangés. L'ajout de `JsonSchema` ne modifie pas les CRDs *déployés* tant qu'ils ne sont pas régénérés. Le schéma *généré par le derive* sera plus précis, mais cela ne prend effet que si les CRDs sont explicitement réappliqués.
- Ne pas migrer les tests HTTP (déjà compatibles avec reqwest 0.12 sans changement de surface mockito).

---

## Context & Research

### Relevant Code and Patterns

- `src/resources.rs` — toutes les structs spec (`InstanceSpec`, `ProjectSpec`, `LoadBalancerSpec`, `NamespaceRoleSpec`) + types imbriqués (`NetworkConfig`, `SecurityConfig`) doivent dériver `JsonSchema`.
- `src/resources.rs:203` — `#[kube(namespaced = false)]` invalide : l'absence de `#[kube(namespaced)]` suffit pour déclarer une ressource cluster-scoped.
- `Cargo.toml` — ajouter `schemars = "1"` et migrer `reqwest = "0.12"`.
- `src/scaleway.rs` — `ReqwestClient::builder()` : vérifier si l'API builder a changé entre 0.11 et 0.12 (mineure, signature identique).

### Institutional Learnings

- Aucun `docs/solutions/` pertinent.

### External References

- [kube-derive 3.x — CustomResource attributes](https://docs.rs/kube-derive/3.1.0/kube_derive/derive.CustomResource.html)
- [kube-rs Schemas documentation](https://kube.rs/controllers/schemas/)
- [reqwest 0.12 changelog](https://docs.rs/reqwest/0.12/reqwest/)

---

## Key Technical Decisions

- **`schemars = "1"` (pas 0.8)** : kube 3.x utilise schemars 1.x. L'ancienne version schemars 0.8 compile mais génère un schéma incompatible.
- **Supprimer `#[kube(namespaced = false)]` plutôt que de le corriger** : la sémantique est "absent = cluster-scoped", ce qui est plus lisible qu'un flag fictif.
- **`reqwest = "0.12"`** : migration mineure (surface API quasi-identique), nécessaire pour éviter des conflits hyper 0.14 / hyper 1.x dans le graphe de dépendances.

---

## Open Questions

### Resolved During Planning

- **`ae.code` fonctionne-t-il encore ?** ⚠️ Type différent — `kube::error::Error::Api` wrappe `Box<k8s_openapi::Status>` en 3.x. `Status.code` est `Option<i32>`, pas `u16`. Les guards `ae.code == 404` et `ae.code == 409` doivent devenir `ae.code == Some(404)` et `ae.code == Some(409)` (littéraux i32). Voir U4.
- **`Action::requeue`, `Action::await_change` ont-ils changé ?** Non — API identique en 3.x.
- **`Controller::new(api, Default::default())` est-il correct ?** Oui — `Default::default()` résout en `watcher::Config::default()`, ce que 3.x attend.

### Deferred to Implementation

- Vérification que `reqwest::ClientBuilder` en 0.12 accepte exactement les mêmes options `.timeout()` / `.connect_timeout()` que 0.11 (très probable, mais à confirmer à la compilation).
- **Vérifier si les structs `*Status` nécessitent `JsonSchema` en kube 3.x** : quand `#[kube(status = "InstanceStatus")]` est présent, le macro peut générer du code de schéma pour la sous-ressource status. Si c'est le cas, `InstanceStatus`, `ProjectStatus`, `NamespaceRoleStatus` devront aussi dériver `JsonSchema`, ce qui implique d'activer le feature `schemars` dans la dépendance `chrono` (`chrono = { ..., features = ["serde", "schemars"] }`).
- **Confirmer que l'absence de `#[kube(namespaced)]` = cluster-scoped en kube 3.x** : si kube 3.x a changé ce défaut, U2 causerait une régression silencieuse (NamespaceRole namespaced, `Api::all()` retourne des résultats incorrects). Vérifier via `cargo doc --open kube_derive` ou le test de compilation que `NamespaceRole` génère bien `scope: Cluster` dans le CRD.

---

## Implementation Units

- U1. **Ajouter `schemars` et `JsonSchema` sur toutes les structs spec**

**Goal:** Satisfaire l'exigence `JsonSchema` de kube 3.x sur chaque struct dérivant `CustomResource`.

**Requirements:** R1, R2

**Dependencies:** Aucune

**Files:**

- Modify: `Cargo.toml`
- Modify: `src/resources.rs`
- Modify: `CONTRIBUTING.md`

**Approach:**

- Ajouter `schemars = "1"` dans `[dependencies]` de `Cargo.toml`.
- Dans `CONTRIBUTING.md`, mettre à jour la version Rust minimale recommandée à 1.80+ (requis par kube 3.x et schemars 1.x).
- Dans `src/resources.rs`, ajouter `schemars::JsonSchema` au `use` ou en chemin complet sur chaque derive.
- Ajouter `JsonSchema` dans `#[derive(...)]` sur : `InstanceSpec`, `NetworkConfig`, `SecurityConfig`, `NamespaceRoleSpec`, `ProjectSpec` (et ses types imbriqués), `LoadBalancerSpec`, `BackendConfig` (type imbriqué de `LoadBalancerSpec.backends`).
- Les structs `*Status` (`InstanceStatus`, `NamespaceRoleStatus`, etc.) n'ont pas besoin de `JsonSchema` — elles ne sont pas dans la spec principale.

**Test scenarios:**

- Test expectation: none — changement structurel pur, vérifié par la compilation. Les tests existants (`cargo test`) confirment que le comportement est préservé.

**Verification:**

- `cargo check` passe sans erreur liée à `JsonSchema`.

---

- U2. **Corriger `#[kube(namespaced = false)]` sur `NamespaceRole`**

**Goal:** Rendre la déclaration cluster-scoped de `NamespaceRole` syntaxiquement valide pour kube 3.x.

**Requirements:** R1, R3

**Dependencies:** Aucune

**Files:**

- Modify: `src/resources.rs`

**Approach:**

- Supprimer la ligne `#[kube(namespaced = false)]` de la définition de `NamespaceRoleSpec`.
- Vérifier qu'aucune autre occurrence de `namespaced = false` n'existe dans le fichier.
- La ressource reste cluster-scoped par défaut (absence de `#[kube(namespaced)]`).

**Test scenarios:**

- Test expectation: none — changement déclaratif pur. La ressource `NamespaceRole` est toujours cluster-scoped ; `Api::all::<NamespaceRole>()` dans `context.rs` continue de fonctionner.

**Verification:**

- `cargo check` passe sans erreur liée au derive macro sur `NamespaceRoleSpec`.
- Le CRD généré (ou le YAML existant `k8s/crd-namespacerole.yaml`) reste `scope: Cluster`.

---

- U3. **Migrer reqwest 0.11 → 0.12**

**Goal:** Éliminer le conflit hyper 0.14 / hyper 1.x introduit par kube 3.x.

**Requirements:** R1, R2

**Dependencies:** Aucune (indépendant de U1/U2)

**Files:**

- Modify: `Cargo.toml`
- Modify: `src/scaleway.rs` (si nécessaire — vérifier à la compilation)
- Verify: `src/error.rs` — contient `NetworkError(#[from] reqwest::Error)` ; `reqwest::Error` garde le même nom en 0.12, probablement sans changement, mais à confirmer à la compilation

**Approach:**

- Dans `Cargo.toml`, changer `reqwest = { version = "0.11", features = ["json"] }` en `"0.12"`.
- reqwest 0.12 est une migration mineure : `ClientBuilder`, `.timeout()`, `.connect_timeout()`, `.header()`, `.json()`, `.send()`, `.status()`, `.text()`, `.json()` ont les mêmes signatures. Aucun changement de code anticipé dans `scaleway.rs`.
- Si `reqwest::StatusCode::NOT_FOUND` ou d'autres constantes ont bougé (improbable), les corriger à la compilation.
- **Vérifier la compatibilité mockito** : mockito < 1.3 utilise hyper 0.14 ; reqwest 0.12 utilise hyper 1.x. S'assurer que la version résolue de mockito est >= 1.3 (qui a migré vers hyper 1.x). Si non, pinner `mockito = "1.3"` dans `[dev-dependencies]`.

**Test scenarios:**

- Test expectation: none — changement de version de dépendance. Les tests HTTP mockito (`cargo test`) valident que le client fonctionne identiquement.

**Verification:**

- `cargo check` passe sans conflit hyper dans le graphe de dépendances.
- `cargo test` passe (tous les tests mockito).

---

## Risks & Dependencies

| Risque | Mitigation |
|--------|------------|
| `schemars 1.x` génère un schéma JSON différent de 0.8 sur certains types (ex: `Option<DateTime>`) | Inspecter le schéma produit avec `cargo run -- --print-schema` ou équivalent si disponible ; comparer avec les CRD YAML existants |
| reqwest 0.12 a changé certains feature flags | Vérifier que le feature `json` est toujours suffisant pour `.json(&body)` et `.json::<Value>()` |
| Types `ProjectSpec`, `LoadBalancerSpec` et leurs imbriqués peuvent avoir des types non-`JsonSchema` (ex: types chrono) | `chrono::DateTime<Utc>` avec feature `serde` est supporté par schemars 1.x via un impl existant |

---

## Documentation / Operational Notes

- Mettre à jour `CONTRIBUTING.md` : la version minimale Rust recommandée passe à 1.80+ (requis par kube 3.x et schemars 1.x).
- Les CRD YAML dans `k8s/` ont été générés manuellement — ils ne changent pas avec cette migration, mais le schéma produit par le derive sera désormais plus précis (les validations OpenAPI seront enrichies automatiquement si les CRDs sont régénérés).

---

- U4. **Corriger les guards `ae.code` dans `context.rs` et `reconcilers.rs`**

**Goal:** Adapter les pattern-match sur `Error::Api` au nouveau type `Box<Status>` de kube 3.x où `code` est `Option<i32>`.

**Requirements:** R1

**Dependencies:** Aucune (indépendant de U1/U2/U3)

**Files:**

- Modify: `src/context.rs`
- Modify: `src/reconcilers.rs`

**Approach:**

- `src/context.rs:54` : changer `ae.code == 404` en `ae.code == Some(404)`.
- `src/reconcilers.rs:123` : changer `ae.code == 409` en `ae.code == Some(409)`.
- Vérifier qu'aucun autre accès à `ae.code` n'existe dans le projet en tant que `u16`.

**Test scenarios:**

- Test expectation: none — changement mécanique de type, vérifié par compilation. Le comportement des guards (404 → ConfigError, 409 → adopter Secret existant) est préservé.

**Verification:**

- `cargo check` passe sans erreur de type sur les guards `ae.code`.

---

## Sources & References

- Versions cibles : `kube = "3.1.0"`, `k8s-openapi = { version = "0.27.1", features = ["v1_35"] }`
- Fichiers impactés : `Cargo.toml`, `src/resources.rs`, `src/scaleway.rs`
- [kube-derive CustomResource doc](https://docs.rs/kube-derive/3.1.0/kube_derive/derive.CustomResource.html)
- [kube-rs schemas guide](https://kube.rs/controllers/schemas/)
