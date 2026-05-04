---
title: "feat: Add unit test coverage for operator core logic"
type: feat
status: active
date: 2026-05-03
---

# feat: Add unit test coverage for operator core logic

## Overview

L'opérateur n'a aucun test en dehors de deux assertions triviales dans `context.rs`. Cette session a introduit du code critique (IAM, réconciliation, gestion d'erreurs HTTP) sans filet de sécurité. Ce plan pose la couverture de test unitaire pour les fonctions directement testables — sans infrastructure Kubernetes ni cluster réel.

---

## Problem Frame

Plusieurs bugs de production ont été découverts par review statique (overflow i32, use-after-move, swallow d'erreurs dans `find_instance_by_name`, boucle de création infinie). Chacun aurait été détecté par un test unitaire de moins de 10 lignes. L'absence de tests ralentit aussi les futures modifications : toute correction implique une validation manuelle.

---

## Requirements Trace

- R1. Les fonctions pures (sans I/O) ont des tests unitaires couvrant happy path, edge cases et cas d'erreur.
- R2. Les méthodes HTTP de `ScalewayClient` sont testables via un serveur mock local, sans appel réel à Scaleway.
- R3. Les chemins d'erreur critiques récemment corrigés ont des tests de régression explicites.
- R4. Le setup de test ne modifie pas le comportement en production.

---

## Scope Boundaries

- Tests d'intégration Kubernetes (`reconcile_instance`, `handle_deletion`, `get_or_provision_namespace_client`) — nécessitent envtest ou k3d, hors scope.
- Tests de bout en bout contre l'API Scaleway réelle — hors scope.
- Couverture de `create_project` / `delete_project` — ces méthodes n'ont pas encore de reconciler, tests à ajouter quand le reconciler Project sera implémenté.

### Deferred to Follow-Up Work

- Tests du reconciler `Project` et `LoadBalancer` : à planifier avec leurs reconcilers respectifs.
- Tests d'intégration avec kube envtest : sujet distinct nécessitant une infrastructure CI dédiée.

---

## Context & Research

### Relevant Code and Patterns

- `src/context.rs:76-98` — seul module de test existant, pattern `#[cfg(test)] mod tests { ... }` à reproduire.
- `src/scaleway.rs` — `SCALEWAY_API_URL` est un `const` hardcodé dans toutes les URLs ; rendre la base URL injectable via un champ `base_url: String` est le prérequis pour les tests HTTP.
- `src/reconcilers.rs:21-33` — `role_to_permission_sets` et `role_allows_write` sont des fonctions pures synchrones, immédiatement testables.
- `src/reconcilers.rs:488-468` — `error_policy` est une fonction synchrone pure, testable sans infrastructure.

### Institutional Learnings

- La review a identifié que `find_instance_by_name` swallait silencieusement les erreurs 403/429 comme `Ok(None)` — ce cas précis doit avoir un test de régression.
- `NamespaceRoleStatus::default()` appelait `Utc::now()` rendant les tests d'égalité non-déterministes — corrigé, mais le test doit vérifier que `last_updated` vaut `None`.
- `verify_project_access` mappait tous les non-2xx sur une erreur permanente — corrigé, le test de régression distingue 403 (permanent) vs 500 (transitoire).

### External References

- `mockito` 1.x API : `Server::new_async().await`, `.mock("METHOD", "/path").with_status(N).create_async().await`. Compatible reqwest 0.11.
- `#[tokio::test]` : fourni par `tokio` déjà en dépendance principale — pas de dep supplémentaire pour les tests async.

---

## Key Technical Decisions

- **`mockito` plutôt que `wiremock`** : mockito 1.x est plus léger, son API est synchrone/async hybride et s'intègre proprement avec reqwest 0.11. wiremock nécessite tokio runtime complet et est plus verbeux pour des tests unitaires simples.
- **Champ `base_url` sur `ScalewayClient`** : ajouter `base_url: String` et un constructeur `new_with_base_url(token, base_url)` pour les tests. `new(token)` continue d'utiliser `SCALEWAY_API_URL`. Toutes les URL dans les méthodes passent de `format!("{}/...", SCALEWAY_API_URL, ...)` à `format!("{}/...", self.base_url, ...)`. Pas d'impact runtime, pas de changement d'interface publique visible.
- **Tests dans `#[cfg(test)] mod tests`** à l'intérieur de chaque fichier source, pas dans un répertoire `tests/` séparé : les fonctions privées (`role_to_permission_sets`, `validate_spec`, etc.) restent accessibles.
- **`tokio-test` retiré** : la crate n'est pas utilisée et crée une fausse impression de couverture. La remplacer par `mockito` suffit pour tous les besoins async.

---

## Open Questions

### Resolved During Planning

- **Faut-il un trait abstrait pour mocker `ScalewayClient` ?** Non — `mockito` intercepte au niveau HTTP, pas besoin de trait. C'est plus simple et teste la vraie sérialisation JSON.
- **Où placer les tests IAM ?** Dans `src/scaleway.rs` avec les autres tests HTTP, même module.

### Deferred to Implementation

- **Noms exacts des champs JSON retournés par l'API IAM Scaleway** : les tests utiliseront des réponses JSON minimalistes basées sur la documentation ; un ajustement peut être nécessaire lors des tests d'intégration réels.

---

## Implementation Units

- U1. **Infrastructure de test : base URL injectable dans ScalewayClient**

**Goal:** Rendre `ScalewayClient` testable en HTTP sans appel réel à Scaleway.

**Requirements:** R2, R4

**Dependencies:** Aucune

**Files:**
- Modify: `src/scaleway.rs`
- Modify: `Cargo.toml`

**Approach:**
- Ajouter `base_url: String` comme champ à `ScalewayClient`.
- Créer `ScalewayClient::new_with_base_url(token: String, base_url: String) -> Self` (constructeur de test, visible dans `#[cfg(test)]` uniquement ou public mais documenté).
- `ScalewayClient::new(token)` délègue à `new_with_base_url(token, SCALEWAY_API_URL.to_string())`.
- Remplacer toutes les occurrences de `SCALEWAY_API_URL` dans les `format!()` par `self.base_url`.
- Dans `Cargo.toml` : remplacer `tokio-test = "0.4"` par `mockito = "1"` dans `[dev-dependencies]`.

**Patterns to follow:**
- `src/context.rs:76` — pattern `#[cfg(test)] mod tests`

**Test scenarios:**
- Test expectation: none — unité purement structurelle, vérifiée par la compilation et les tests des unités suivantes.

**Verification:**
- `cargo check` passe sans erreur.
- `ScalewayClient::new("tok".into())` continue d'utiliser `https://api.scaleway.com` comme base URL.

---

- U2. **Tests des fonctions pures**

**Goal:** Couvrir toutes les fonctions sans I/O : helpers de context, validation, helpers IAM, error_policy, defaults de resources.

**Requirements:** R1, R3

**Dependencies:** Aucune (pas besoin de U1)

**Files:**
- Modify: `src/context.rs` (étendre le module de test existant)
- Modify: `src/scaleway.rs` (ajouter `#[cfg(test)] mod tests`)
- Modify: `src/reconcilers.rs` (ajouter `#[cfg(test)] mod tests`)
- Modify: `src/resources.rs` (ajouter `#[cfg(test)] mod tests`)

**Approach:**
- Étendre `context.rs` : ajouter test manquant pour `extract_org_id_from_namespace`.
- `scaleway.rs` : `validate_zone` et `validate_instance_type` sont `async fn` sans await — utiliser `#[tokio::test]`.
- `reconcilers.rs` : `role_to_permission_sets`, `role_allows_write` sont synchrones. `error_policy` prend un `Arc<Instance>` — construire un objet minimal ou extraire la logique dans une fonction pure testable directement.
- `resources.rs` : `InstanceStatus::default()` et `NamespaceRoleStatus::default()` — assertions sur les valeurs initiales.

**Test scenarios:**

*context.rs — `extract_org_id_from_namespace` :*
- Happy path : annotation `scaleway.io/organization-id` présente → `Some(valeur)`
- Edge case : annotation absente → `None`
- Edge case : BTreeMap vide → `None`

*scaleway.rs — `validate_zone` :*
- Happy path : `"fr-par-1"` → `Ok(())`
- Happy path : `"it-mil-1"` → `Ok(())`
- Error path : zone inconnue `"us-east-1"` → `Err(InvalidZone("us-east-1"))`
- Edge case : string vide `""` → `Err(InvalidZone(""))`

*scaleway.rs — `validate_instance_type` :*
- Happy path : `"DEV1-S"` → `Ok(())`
- Error path : type inconnu `"MEGA-XL"` → `Err(InvalidInstanceType("MEGA-XL"))`

*reconcilers.rs — `role_to_permission_sets` :*
- `"Editor"` → `["InstancesFullAccess"]`
- `"Admin"` → `["InstancesFullAccess"]`
- `"Viewer"` → `["InstancesReadOnly"]`
- `"BillingManager"` → `["InstancesReadOnly"]`
- Rôle inconnu `"Wizard"` → `["InstancesReadOnly"]` (default conservatif)

*reconcilers.rs — `role_allows_write` :*
- `"Editor"` → `true`
- `"Admin"` → `true`
- `"OrganizationOwner"` → `true`
- `"Viewer"` → `false`
- `"BillingViewer"` → `false`
- Rôle inconnu → `false`

*reconcilers.rs — `error_policy` :*
- `ConfigError(...)` → `Action::await_change()` (erreur permanente)
- `InvalidZone(...)` → `Action::await_change()`
- `ProjectAccessDenied(...)` → `Action::await_change()`
- `NetworkError(...)` → `Action::requeue(60s)` (erreur transitoire)
- `ScalewayError { ... }` → `Action::requeue(60s)`

*resources.rs — defaults :*
- `InstanceStatus::default()` : `scaleway_id = None`, `state = "unknown"`, `sync_state = "Syncing"`
- `NamespaceRoleStatus::default()` : `last_updated = None` (régression : ne doit plus appeler `Utc::now()`)

**Verification:**
- `cargo test` passe pour les modules `context::tests`, `scaleway::tests`, `reconcilers::tests`, `resources::tests`.

---

- U3. **Tests HTTP happy path de ScalewayClient**

**Goal:** Vérifier que chaque méthode HTTP sérialise correctement la requête et désérialise la réponse.

**Requirements:** R2, R3

**Dependencies:** U1

**Files:**
- Modify: `src/scaleway.rs` (dans le module `#[cfg(test)]`)

**Approach:**
- Chaque test démarre un `mockito::Server::new_async().await`, enregistre un mock de réponse JSON, instancie `ScalewayClient::new_with_base_url("test-token", server.url())`, appelle la méthode, vérifie le résultat et que le mock a bien été appelé (`.assert_async().await`).
- Les corps de réponse JSON sont les structures minimales attendues par le parsing actuel.

**Test scenarios:**

*`create_instance` :*
- Happy path : serveur répond 201 avec `{"server": {"id": "srv-123"}}` → `Ok("srv-123")`
- Happy path : les champs `name`, `commercial_type`, `project_id` sont présents dans le body de la requête (vérifiable avec `.match_body(mockito::Matcher::PartialJsonString(...))`)

*`find_instance_by_name` :*
- Happy path : serveur répond 200 avec `{"servers": [{"id": "srv-abc"}]}` → `Ok(Some("srv-abc"))`
- Happy path : serveur répond 200 avec `{"servers": []}` → `Ok(None)`
- Happy path : les query params `project_id` et `name` sont URL-encodés correctement (nom avec espace `"my server"` → `name=my+server` ou `name=my%20server`)

*`get_instance` :*
- Happy path : 200 avec state et public_ip → `Ok(InstanceInfo { state: "running", ... })`

*`delete_instance` :*
- Happy path : 204 → `Ok(())`
- Happy path (régression) : 404 → `Ok(())` — l'instance déjà supprimée ne doit pas retourner une erreur

*`verify_project_access` :*
- Happy path : 200 → `Ok(())`

*`find_iam_application_by_name` :*
- Happy path : 200 avec `{"applications": [{"id": "app-1", "name": "scaleway-operator-prod"}]}` → `Ok(Some("app-1"))`
- Happy path : 200 avec `{"applications": []}` → `Ok(None)`

*`create_iam_application` :*
- Happy path : 200 avec `{"id": "app-2"}` → `Ok("app-2")`

*`create_iam_api_key` :*
- Happy path : 200 avec `{"access_key": "AK...", "secret_key": "SK..."}` → `Ok(("AK...", "SK..."))`

**Verification:**
- `cargo test scaleway::tests` passe.
- Chaque mock est vérifié avec `.assert_async().await` pour confirmer que la requête a bien été émise.

---

- U4. **Tests HTTP error path — régressions critiques**

**Goal:** Fixer comme tests de non-régression les bugs de gestion d'erreur HTTP corrigés pendant cette session.

**Requirements:** R3

**Dependencies:** U1

**Files:**
- Modify: `src/scaleway.rs` (dans le module `#[cfg(test)]`)

**Approach:**
- Même pattern que U3 avec mockito, mais les réponses sont des codes d'erreur.
- Ces tests documentent explicitement les comportements qui avaient été rompus.

**Test scenarios:**

*`find_instance_by_name` — régression principale :*
- Error path : serveur répond **403** → `Err(ScalewayError { status: "403 Forbidden", ... })` — pas `Ok(None)`. Ce test aurait détecté le bug de création de doublons.
- Error path : serveur répond **429** → `Err(ScalewayError { ... })` — pas `Ok(None)`
- Error path : serveur répond **500** → `Err(ScalewayError { ... })` — pas `Ok(None)`
- Edge case : serveur répond **404** → `Ok(None)` (seul cas légitime de "pas trouvé")

*`get_instance` :*
- Error path : **404** → `Err(InstanceNotFound("srv-xyz"))`
- Error path : **401** → `Err(ScalewayError { status: "401 Unauthorized", ... })` — distinct de InstanceNotFound

*`verify_project_access` — régression error_policy :*
- Error path : **403** → `Err(ProjectAccessDenied(...))` — erreur permanente (await_change)
- Error path : **404** → `Err(ConfigError("Project 'proj-x' not found"))` — erreur permanente
- Error path : **500** → `Err(ScalewayError { ... })` — erreur transitoire (requeue 60s)
- Error path : **429** → `Err(ScalewayError { ... })` — erreur transitoire

*`create_instance` — use-after-move (régression compile) :*
- Error path : serveur répond **422** → `Err(ScalewayError { status: "422 Unprocessable Entity", message: "<body>" })`. Vérifie que `status` et `message` sont tous deux correctement capturés (régression du use-after-move fixé).

*`delete_instance` :*
- Error path : **403** → `Err(ScalewayError { ... })`

**Verification:**
- `cargo test scaleway::tests` passe.
- Tout test qui passerait avec l'ancien code swallowing `Ok(None)` doit échouer si on revient au code d'avant.

---

## System-Wide Impact

- **Interaction graph :** U1 modifie la signature interne de `ScalewayClient` mais pas son interface publique visible de `reconcilers.rs` ou `main.rs`.
- **État lifecycle :** `new_with_base_url` n'est pas exposé en prod — uniquement dans les tests. Le const `SCALEWAY_API_URL` reste la valeur de production.
- **API surface parity :** `Context` contient un `ScalewayClient` — après U1, le champ `scaleway_client` dans `Context` est construit via `new()` qui utilise toujours le const. Aucun changement nécessaire dans `context.rs` ou `main.rs`.
- **Invariants préservés :** L'URL de production `https://api.scaleway.com` reste hardcodée dans `new()`. Aucun risque de redirection vers un mock en prod.

---

## Risks & Dependencies

| Risque | Mitigation |
|--------|------------|
| `mockito` 1.x et `reqwest` 0.11 : incompatibilité de runtime async | Mockito 1.x supporte reqwest 0.11 nativement ; la doc le confirme. À valider à la compilation. |
| Les URLs mockito incluent `http://127.0.0.1:<port>` — les tests qui vérifient l'URL exacte dans le body pourraient être fragiles | Ne pas asserter sur l'URL complète dans les tests, seulement sur le chemin. |
| `error_policy` prend `Arc<Instance>` — construire l'objet peut nécessiter des champs obligatoires | Utiliser `Default::default()` via la derive kube, ou ignorer le paramètre instance dans le test (il n'est pas utilisé dans la logique actuelle). |

---

## Sources & References

- Fonctions pures testables identifiées : `src/context.rs:14,23`, `src/scaleway.rs:478,495`, `src/reconcilers.rs:21,32,488`
- Tests existants à étendre : `src/context.rs:76-98`
- mockito 1.x : https://docs.rs/mockito/latest/mockito/
- Bugs couverts par U4 : swallow d'erreurs `find_instance_by_name` (corrigé), `verify_project_access` non-2xx permanent (corrigé), use-after-move sur `response.status()` (corrigé)
