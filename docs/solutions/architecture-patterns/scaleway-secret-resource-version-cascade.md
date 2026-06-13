---
module: scaleway-secret
tags: [resource-version, scaleway-api-cost, rotation-detection]
problem_type: design-decision
---

# ScalewaySecret : pourquoi resourceVersion peut produire des révisions inutiles

## Trade-off accepté

`status.last_synced_resource_version` est comparé au `resourceVersion` du
Secret K8s source pour décider si une nouvelle version Scaleway doit être
créée. Le `resourceVersion` change sur **toute** modification du Secret K8s —
y compris les changements de métadonnées (labels, annotations, finalizers)
qui ne touchent pas la valeur.

À grande échelle (GitOps ArgoCD/Flux qui re-applique des labels toutes les N
minutes), cela peut produire des révisions Scaleway identiques superflues —
quota et coût.

## Alternative étudiée puis rejetée

HMAC de la valeur avec clé persistée :

- Pro : détection exacte des changements de valeur
- Con : gestion de clé (Secret K8s dédié, lifecycle helm, rotation),
  nouvelle surface de risque sécurité
- Verdict : trop d'infrastructure pour le bénéfice. La fix SEC-002 (oracle
  hash) excluait déjà SHA-256 sans clé.

## Mitigation actuelle

Aucune côté code. Coté ops : surveiller
`scaleway_operator_reconcile_total{outcome="Synced"}` et le compteur de
versions créées côté Scaleway. Si dérive observée → arbitrer avec le team
plate-forme s'il faut introduire HMAC.

## Lien

- Issue #118 (PR #113 et suivantes)
- SEC-002 closes : remplacement hash SHA-256 par resourceVersion
