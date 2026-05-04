---
title: "Privilege escalation par provisionnement dynamique IAM dans l'opérateur Scaleway"
date: 2026-05-03
category: docs/solutions/security-issues/
module: reconciler
problem_type: security_issue
component: service_object
severity: high
symptoms:
  - "L'opérateur crée des IAM Applications, Policies et API Keys au runtime"
  - "Le token global de l'opérateur requiert IAMManager (org-scope) en plus de InstancesFullAccess"
  - "Un opérateur compromis peut s'octroyer n'importe quelle permission Scaleway"
root_cause: scope_issue
resolution_type: code_fix
tags:
  - iam
  - privilege-escalation
  - kubernetes-operator
  - scaleway-iam
  - security
  - secret
---

# Privilege escalation par provisionnement dynamique IAM dans l'opérateur Scaleway

## Problem

L'opérateur créait dynamiquement des IAM Applications, Policies et API Keys Scaleway au moment de la réconciliation.
Cette approche requiert que le token global de l'opérateur dispose de la permission `IAMManager` (org-scope)ce qui lui permet de s'octroyer n'importe quelle permission Scaleway — une faille de privilege escalation.

## Symptoms

- La fonction `get_or_provision_namespace_client` dans `reconcilers.rs` appelle `create_iam_application`, `create_iam_policy`, `create_iam_api_key` via le `ScalewayClient` global de l'opérateur
- Le token global de l'opérateur nécessite `IAMManager` (scope organisation) en plus de `InstancesFullAccess` et `ProjectReadOnly`
- Un attaquant ayant compromis le pod opérateur peut créer une IAM Policy avec `AllProductsFullAccess` sur toute l'organisation

## What Didn't Work

Le problème n'a pas été détecté lors de l'implémentation initiale du pattern NamespaceRole : le provisionnement dynamique semblait être le moyen naturel d'associer des credentials scopés à chaque namespace.
Ce n'est qu'en questionnant les permissions réellement requises (`IAMManager` vs `IAMApplicationManager` + `IAMPolicyManager`) que la faille de conception a été identifiée : un opérateur qui peut créer des policies IAM est son propre administrateur de sécurité.

## Solution

Remplacer `get_or_provision_namespace_client` (~105 lignes) par `get_namespace_client` (~20 lignes) qui lit un Secret Kubernetes pré-provisionné par un admin :

**Avant — provisionnement dynamique (supprimé) :**

```rust
// L'opérateur crée lui-même ses credentials IAM → privilege escalation
async fn get_or_provision_namespace_client(
    ctx: &Arc<Context>,
    namespace: &str,
    scaleway_role: &str,
    project_id: &str,
) -> Result<ScalewayClient> {
    // ...cherche ou crée IAM Application...
    // ...cherche ou crée IAM Policy...
    // ...crée une API Key...
    // ...stocke dans un Secret K8s...
}
```

**Après — lecture d'un Secret pré-provisionné :**

```rust
/// L'admin crée scaleway-ns-creds-{namespace} dans scaleway-system.
/// L'opérateur lit, ne crée jamais rien côté IAM.
async fn get_namespace_client(
    ctx: &Arc<Context>,
    namespace: &str,
) -> Result<ScalewayClient> {
    let secret_name = format!("scaleway-ns-creds-{}", namespace);
    let secrets_api: Api<Secret> = Api::namespaced(ctx.client.clone(), NAMESPACE_CREDS_NS);

    let secret = secrets_api.get(&secret_name).await.map_err(|_| {
        OperatorError::ConfigError(format!(
            "Secret '{secret_name}' not found in namespace '{NAMESPACE_CREDS_NS}'. \
             An admin must pre-provision IAM credentials for this namespace.",
        ))
    })?;

    let secret_key = secret
        .data
        .as_ref()
        .and_then(|d| d.get("secret_key"))
        .ok_or_else(|| OperatorError::ConfigError(format!(
            "Secret '{secret_name}' has no 'secret_key' field.",
        )))
        .and_then(|bytes| {
            String::from_utf8(bytes.0.clone()).map_err(|_| {
                OperatorError::ConfigError(format!(
                    "Secret '{secret_name}': 'secret_key' is not valid UTF-8.",
                ))
            })
        })?;

    Ok(ScalewayClient::new(secret_key))
}
```

**Supprimé de `scaleway.rs` :**

- `find_iam_application_by_name`
- `find_iam_policy_by_application`
- `create_iam_application`
- `create_iam_api_key`
- `create_iam_policy`
- `role_to_permission_sets` (dans `reconcilers.rs`)

**Permissions requises après le fix :**

| Permission            | Scope        | Raison                             |
|-----------------------|--------------|------------------------------------|
| `InstancesFullAccess` | projet       | Créer/supprimer/lire les instances |
| `ProjectReadOnly`     | organisation | Vérifier l'accès au projet         |

Zero permission IAM pour le token global de l'opérateur.

**Convention Secret admin :**

```bash
# L'admin crée manuellement le Secret par namespace
kubectl create secret generic scaleway-ns-creds-production \
  --from-literal=secret_key=<IAM_API_KEY_SECRET> \
  -n scaleway-system
```

L'IAM Application + Policy + API Key sont créées une fois par un admin (Terraform, console, CI) avec les permissions appropriées au rôle du namespace.

## Why This Works

Un opérateur qui ne peut que lire des Secrets Kubernetes ne peut pas s'octroyer de permissions supplémentaires.
La création de credentials IAM reste sous contrôle humain (admin cluster ou pipeline IaC), hors de portée d'un pod compromis.
Le modèle de menace passe de "l'opérateur peut devenir admin Scaleway" à "l'opérateur peut lire les Secrets qu'un admin lui a explicitement accordés".

## Prevention

- Ne jamais accorder à un opérateur la permission de créer des policies IAM dans le cloud provider qu'il gère — c'est un anti-pattern de privilege escalation systématique.
- Le pattern correct : les credentials sont créés hors-bande (Terraform, console admin, CI) et injectés dans des Secrets Kubernetes. L'opérateur lit, ne crée pas.
- Lors de la revue des permissions requises par un opérateur, tout besoin de `*Manager` ou `*FullAccess` sur IAM doit être un signal d'alarme à investiguer.

## Related Issues

- `src/reconcilers.rs` — `get_namespace_client()` (nouveau), anciennement `get_or_provision_namespace_client()`
- `src/scaleway.rs` — section IAM supprimée
- `docs/solutions/architecture-patterns/namespacerole-namespace-annotation-scaleway-multiproject-2026-05-03.md` — pattern NamespaceRole (context de ce fix)
- `SETUP.md` — mis à jour avec les permissions correctes (sans `IAMManager`)
