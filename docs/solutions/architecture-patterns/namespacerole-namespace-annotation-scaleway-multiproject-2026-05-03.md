---
title: "NamespaceRole CRD et annotation namespace pour la ségrégation multi-projet dans un opérateur Kubernetes Scaleway"
date: 2026-05-03
category: docs/solutions/architecture-patterns/
module: reconciler
problem_type: architecture_pattern
component: service_object
severity: high
applies_when:
  - L'opérateur gère des ressources dans plusieurs namespaces avec des projets Scaleway distincts
  - Les équipes applicatives ne doivent pas avoir accès aux credentials de l'organisation
  - Des niveaux de permission différents coexistent dans le même cluster (prod Editor, staging Viewer)
  - La création de ressources doit être conditionnée à une autorisation explicite par namespace
tags:
  - kubernetes-operator
  - rust
  - kube-rs
  - namespace-isolation
  - scaleway-iam
  - crd
  - multi-project
  - multi-tenant
---

# NamespaceRole CRD et annotation namespace pour la ségrégation multi-projet dans un opérateur Kubernetes Scaleway

## Context

Dans un opérateur Kubernetes multi-tenant gérant des ressources Scaleway, deux frictions architecturales se posent rapidement :

**Friction 1 — La répétition du `project_id`.** Sans convention centralisée, chaque ressource `Instance` devrait déclarer son `project_id` dans sa `spec`. En environnement multi-équipe, cela crée de la duplication, des erreurs de copier-coller, et rend difficile la réaffectation d'un namespace vers un projet Scaleway différent.

**Friction 2 — L'absence de contrôle de permission par tenant.** L'opérateur dispose d'un token Scaleway global (admin). Sans gouvernance, toute équipe déployant dans n'importe quel namespace peut déclencher des appels API avec les permissions complètes. Il n'y a pas de moyen d'exprimer qu'un namespace de développement ne doit avoir accès qu'en lecture, ou qu'un namespace de production peut créer des instances mais pas modifier l'IAM.

Ces deux problèmes sont résolus par des conventions portées par des primitives Kubernetes natives — annotations et CRDs — plutôt que par de la configuration dans la `spec` des ressources métier.

## Guidance

### Pattern 1 : Annotation `scaleway.io/project-id` sur le namespace

Le projet Scaleway cible est déclaré une seule fois, sur le namespace, via une annotation. Toutes les ressources `Instance` du namespace héritent implicitement de ce contexte de projet.

**Configuration du namespace :**

```yaml
apiVersion: v1
kind: Namespace
metadata:
  name: production
  annotations:
    scaleway.io/project-id: "12345678-1234-1234-1234-123456789012"
```

**Extraction dans `context.rs` :**

```rust
const SCALEWAY_PROJECT_ANNOTATION: &str = "scaleway.io/project-id";

pub fn extract_project_id_from_namespace(
    namespace_annotations: &std::collections::BTreeMap<String, String>,
) -> Option<String> {
    namespace_annotations
        .get(SCALEWAY_PROJECT_ANNOTATION)
        .cloned()
}
```

L'annotation est validée en tant qu'UUID avant tout appel API, pour prévenir les injections dans les URLs Scaleway :

```rust
if uuid::Uuid::parse_str(&pid).is_err() {
    return Err(OperatorError::ConfigError(format!(
        "Annotation 'scaleway.io/project-id' must be a valid UUID, got: '{}'",
        pid
    )));
}
```

### Pattern 2 : CRD `NamespaceRole` cluster-wide

La CRD `NamespaceRole` est une ressource cluster-wide (non-namespaced) dont **le nom de la ressource est identique au nom du namespace** qu'elle configure. Cette convention élimine tout champ de sélecteur : le lookup se fait directement par `api.get(namespace_name)`.

**Struct Rust (`src/resources.rs`) :**

```rust
#[derive(CustomResource, Serialize, Deserialize, Debug, Clone, JsonSchema)]
#[kube(group = "scaleway.io", version = "v1", kind = "NamespaceRole")]
#[kube(status = "NamespaceRoleStatus")]
// Absence de #[kube(namespaced)] → cluster-wide
pub struct NamespaceRoleSpec {
    pub namespace: String,
    pub scaleway_role: String,
    #[serde(default)]
    pub description: Option<String>,
}
```

**Lookup dans `src/context.rs` :**

```rust
pub async fn get_scaleway_role_for_namespace(
    client: &Client,
    namespace: &str,
) -> crate::error::Result<String> {
    let api: Api<NamespaceRole> = Api::all(client.clone()); // cluster-wide

    match api.get(namespace).await { // Convention: nom = namespace
        Ok(ns_role) => Ok(ns_role.spec.scaleway_role.clone()),
        Err(kube::error::Error::Api(ae)) if ae.code == 404 => {
            Err(crate::error::OperatorError::ConfigError(format!(
                "No NamespaceRole found for namespace '{}'. \
                 Create a NamespaceRole resource with name '{}' to assign a Scaleway role.",
                namespace, namespace
            )))
        }
        Err(e) => Err(crate::error::OperatorError::KubeError(e)),
    }
}
```

### Intégration dans le flux de réconciliation

Les deux lookups constituent les premières étapes de `reconcile_instance`, avant tout travail sur l'instance elle-même :

```rust
pub async fn reconcile_instance(
    instance: Arc<Instance>,
    ctx: Arc<Context>,
) -> std::result::Result<Action, OperatorError> {
    // Suppression vérifiée en priorité (avant les lookups)
    if instance.metadata.deletion_timestamp.is_some() {
        return handle_deletion(...).await;
    }

    // Étape 1 : Rôle Scaleway depuis NamespaceRole (nom = namespace)
    let scaleway_role = get_scaleway_role_for_namespace(&ctx.client, &namespace).await?;

    // Étape 2 : project_id depuis l'annotation du namespace
    let project_id = get_project_id_from_namespace(&instance, &ctx).await?;

    // Étapes suivantes : finalizer, validation, provisioning IAM, create/sync
    // ... scaleway_role et project_id sont propagés à toutes les opérations
}
```

Le `scaleway_role` est traduit en `permission_sets` Scaleway IAM :

```rust
fn role_to_permission_sets(role: &str) -> &'static [&'static str] {
    match role {
        "Editor" | "Admin" | "OrganizationOwner" => &["InstancesFullAccess"],
        "Viewer" | "SecurityResponsible" | "BillingViewer" | "BillingManager" => &["InstancesReadOnly"],
        _ => &["InstancesReadOnly"], // défaut conservatif
    }
}

fn role_allows_write(role: &str) -> bool {
    matches!(role, "Editor" | "Admin" | "OrganizationOwner")
}
```

## Why This Matters

**Isolation du périmètre de permission.** Plutôt qu'un token global avec accès admin sur toute l'organisation, chaque namespace dispose de credentials IAM Scaleway dédiés, provisionnés à la volée et bornés aux `permission_sets` correspondant à son rôle. Un namespace `Viewer` ne peut pas créer d'instances, même si un développeur modifie la `spec` d'une `Instance`.

**Convention over configuration.** Le lookup `api.get(namespace)` est O(1) et sans ambiguïté : un namespace, un `NamespaceRole`, une résolution. Pas de label selector, pas de champ de référence à synchroniser.

**Séparation des préoccupations multi-tenant.** Le `project_id` et le rôle IAM relèvent de la gouvernance de la plateforme (opérateur cluster-admin), pas des équipes applicatives. En les portant sur des objets cluster-wide (`NamespaceRole`) ou des annotations namespace, on préserve cette frontière.

**Fail-fast sur configuration manquante.** Les deux lookups sont des préconditions non-nullables. Une `ConfigError` déclenche `Action::await_change()` (pas de requeue temporisé) : le cluster ne consomme pas de ressources à retenter une configuration structurellement incorrecte.

## When to Apply

- Utiliser ces patterns dès qu'il y a **plusieurs namespaces avec des projets Scaleway distincts**.
- Les utiliser ensemble — NamespaceRole sans annotation namespace perd le contexte de projet ; annotation sans NamespaceRole perd le contrôle d'accès.
- Ne pas appliquer si tous les namespaces utilisent le même projet et le même niveau de permission : une variable d'environnement globale `SCALEWAY_PROJECT_ID` suffit et évite la complexité opérationnelle.

## Examples

### Namespace "production" — Editor, création autorisée

```yaml
# 1. Annoter le namespace
apiVersion: v1
kind: Namespace
metadata:
  name: production
  annotations:
    scaleway.io/project-id: "12345678-1234-1234-1234-123456789012"
---
# 2. Créer le NamespaceRole (nom = namespace — convention stricte)
apiVersion: scaleway.io/v1
kind: NamespaceRole
metadata:
  name: production
spec:
  namespace: production
  scaleway_role: Editor
  description: "Production — Editor, création d'instances autorisée"
---
# 3. Instance sans project_id (hérité du namespace)
apiVersion: scaleway.io/v1
kind: Instance
metadata:
  name: web-server
  namespace: production
spec:
  name: web-server-prod
  zone: fr-par-1
  image: ubuntu-jammy
  instance_type: GP1-M
```

À la réconciliation, l'opérateur :

1. Résout `NamespaceRole/production` → rôle `Editor`
2. Résout l'annotation `scaleway.io/project-id` → `12345678-...`
3. Vérifie `role_allows_write("Editor")` → `true`
4. Provisionne IAM Application + Policy `InstancesFullAccess` + API Key
5. Crée l'instance avec les credentials scopés

### Namespace "staging" — Viewer, lecture seule

```yaml
apiVersion: v1
kind: Namespace
metadata:
  name: staging
  annotations:
    scaleway.io/project-id: "aaaabbbb-cccc-dddd-eeee-ffffffffffff"
---
apiVersion: scaleway.io/v1
kind: NamespaceRole
metadata:
  name: staging
spec:
  namespace: staging
  scaleway_role: Viewer
```

Toute `Instance` dans ce namespace sera bloquée à la création avec :
`Role 'Viewer' is read-only and cannot create instances. Use 'Editor' or 'Admin'.`

## Ce qui n'a pas marché (session history)

- **Import incorrect du type `Namespace`** — Le premier code utilisait `kube::api::v1::Namespace` qui n'existe pas dans kube-rs. Le chemin correct est `k8s_openapi::api::core::v1::Namespace`. Cette erreur bloquait la compilation. (session history)
- **`scaleway_role` lu mais non appliqué** — La version initiale lisait le champ `scaleway_role` depuis `NamespaceRole` mais continuait à utiliser le `ScalewayClient` global de l'opérateur sans tenir compte du rôle. Identifié comme bypass de sécurité critique en revue de code multi-persona, puis corrigé en introduisant `get_or_provision_namespace_client`. (session history)
- **Appels IAM Scaleway en 404** — Pendant la phase d'implémentation IAM, des appels `find_iam_application_by_name` retournaient 404 (l'application n'existait pas encore). Des cas limites produisaient des erreurs silencieuses menant à des créations dupliquées d'Applications orphelines. Résolu en gérant explicitement les 409 (race condition) avec un re-lookup plutôt qu'une propagation d'erreur. (session history)

## Related

- `src/context.rs` — `get_scaleway_role_for_namespace()`, `extract_project_id_from_namespace()`
- `src/resources.rs` — struct `NamespaceRoleSpec`, CRD definition
- `src/reconcilers.rs` — `reconcile_instance()`, `role_to_permission_sets()`, `role_allows_write()`
- `k8s/crd-namespacerole.yaml` — manifeste CRD Kubernetes
- `NAMESPACE_ROLES.md` — guide opérationnel complet (cas d'usage, troubleshooting, FAQ)
- `NAMESPACE_PROJECTS.md` — guide opérationnel annotation `scaleway.io/project-id`
