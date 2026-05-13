---
name: Scaleway Kubernetes Operator
last_updated: 2026-05-13
---

# Scaleway Kubernetes Operator Strategy

## Problème cible

Les équipes ops/platform qui gèrent de l'infrastructure Scaleway s'appuient sur Terraform, mais son modèle d'état externe crée des incohérences entre plan et apply, un tfstate fragile, et des providers en retard sur l'API — en plus d'introduire une dépendance tierce à risque supply-chain. Le nœud du problème : l'état souhaité vit en dehors du cluster, dans un outil avec son propre langage et ses propres modes de défaillance, alors que le cluster dispose déjà d'une primitive de réconciliation d'état éprouvée.

## Notre approche

Remplacer Terraform par un opérateur Kubernetes natif comme mécanisme de gestion de l'infrastructure Scaleway — le cluster devient l'unique plan de contrôle pour les workloads et les ressources cloud qu'ils consomment, exprimés en YAML, réconciliés en continu.

## Pour qui

**Principale cible :** Ops/platform engineers responsables de l'infrastructure Scaleway — ils confient à l'opérateur la gestion déclarative des ressources cloud depuis leur cluster, sans toolchain séparée ni gestion manuelle d'état.

## Métriques clés

- **Délai médian de provisionnement** — temps entre la soumission d'une CR et l'état Running/succès côté Scaleway ; mesuré via les métriques Prometheus de l'opérateur
- **Taux d'erreurs de réconciliation** — erreurs / total de cycles de réconciliation par semaine ; mesuré via `scaleway_operator_reconcile_errors_total`
- **Couverture fonctionnelle de l'API Scaleway** — % des types de ressources Scaleway demandés qui ont une CRD disponible ; suivi manuellement au fil des releases
- **Adoption par type de ressource** — nombre d'instanciations actives par CRD ; mesuré via les métriques Prometheus de l'opérateur

## Axes de travail

### Couverture API Scaleway

Fournir un ensemble cohérent de CRDs permettant le déploiement d'infrastructure Scaleway depuis Kubernetes.

_Lien avec l'approche :_ Sans couverture suffisante de l'API, les équipes restent dépendantes de Terraform pour les ressources non couvertes — l'opérateur ne peut pas remplacer entièrement la toolchain.

### Fiabilité de l'opérateur

Garantir un opérateur robuste et pérenne qui facilite le diagnostic des problèmes et simplifie la mise en place d'infrastructures stables.

_Lien avec l'approche :_ Un opérateur peu fiable invalide le pari central — les équipes reviendraient à Terraform faute de pouvoir faire confiance à la réconciliation.

### Expérience développeur

Permettre aux équipes de contribuer et d'étendre l'opérateur facilement.

_Lien avec l'approche :_ L'extensibilité native de Kubernetes n'a de valeur que si le code de l'opérateur est accessible — c'est ce qui permet à la communauté de couvrir de nouveaux cas d'usage sans dépendre d'un mainteneur unique.

### Mise en place et documentation

Rendre l'opérateur opérationnel en moins de 20 minutes.

_Lien avec l'approche :_ Si l'onboarding est long ou opaque, les équipes n'adoptent pas l'opérateur et restent sur leurs pratiques existantes.
