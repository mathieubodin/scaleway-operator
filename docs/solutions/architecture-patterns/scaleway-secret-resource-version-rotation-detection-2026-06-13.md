---
title: "Détection de rotation ScalewaySecret via resourceVersion : trade-off accepté"
date: 2026-06-13
category: docs/solutions/architecture-patterns/
module: scaleway-secret
problem_type: design-decision
component: reconcile_scaleway_secret
severity: low
applies_when:
  - Un ScalewaySecret synchronise une valeur depuis un Secret K8s vers Scaleway Secret Manager
  - Le Secret K8s source est manipulé par un autre contrôleur (GitOps, sealed-secrets, kustomize)
  - On veut éviter d'introduire un HMAC avec clé persistée pour la détection de rotation
tags:
  - kubernetes-operator
  - rust
  - scaleway-secret-manager
  - resource-version
  - rotation-detection
  - scaleway-api-cost
---

# Détection de rotation ScalewaySecret via resourceVersion

## Pourquoi resourceVersion plutôt qu'un hash de la valeur ?

L'opérateur compare `status.last_synced_resource_version` au `metadata.resourceVersion` du Secret K8s source pour décider s'il faut pousser une nouvelle version sur Scaleway Secret Manager.

Cette approche a été retenue **après** que la première implémentation (hash SHA-256 de la valeur stocké dans le status) a été identifiée comme un oracle de pré-image lors de la revue Opus de la PR #113 (issue SEC-002).
Conserver un dérivé déterministe de la valeur dans un champ lisible par tout user avec accès `get scalewaysecrets/status` permet à un attaquant de tester ses hypothèses sur des valeurs faibles ou structurées.

resourceVersion ne porte aucune information sur la valeur, ne dépend que de l'ordonnancement etcd, et reste comparable d'un reconcile à l'autre — c'est le compromis le plus simple qui ferme l'oracle.

## Trade-off accepté

Le `resourceVersion` change sur **toute** modification du Secret K8s, y compris les changements de métadonnées (labels, annotations, finalizers) qui ne touchent pas la valeur.

À grande échelle (GitOps ArgoCD/Flux qui re-applique des labels toutes les N minutes), cela peut produire des révisions Scaleway identiques superflues — quota et coût. La probabilité grandit avec le nombre de ScalewaySecret synchronisés et la cadence d'un éventuel reconciler annexe.

## Alternative étudiée puis rejetée

HMAC de la valeur avec clé persistée :

- Pro : détection exacte des changements de valeur, sans oracle (la clé HMAC reste interne à l'opérateur).
- Con : gestion d'une nouvelle Secret K8s dédiée à la clé, lifecycle helm (création à l'install, garde à l'upgrade), rotation périodique, sauvegarde — une nouvelle surface de risque sécurité non négligeable.
- Verdict : trop d'infrastructure pour un bénéfice borné. Le correctif SEC-002 a tranché en faveur de resourceVersion.

## Mitigation actuelle

Aucune côté code. Côté ops, surveiller :

- `scaleway_operator_reconcile_total{outcome="Synced"}` (rythme de synchronisation)
- Le compteur de versions créées côté Scaleway (via la console ou l'API listing)
- L'absence de bursts corrélés à des opérations GitOps connues

Si une dérive est observée → arbitrer avec l'équipe plateforme s'il faut introduire HMAC.

## Liens

- Issue [#118](https://github.com/mathieubodin/scaleway-operator/issues/118) — origine de la trace de cette décision.
- PR #113 — implémentation initiale (SEC-002) puis durcissements.
- `docs/solutions/security-issues/` — voisinage thématique sur les arbitrages sécurité/coût.
