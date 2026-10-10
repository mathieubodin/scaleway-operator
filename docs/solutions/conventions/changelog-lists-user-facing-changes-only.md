---
title: "Un changelog ne liste que ce qui change pour un utilisateur"
date: 2026-10-10
category: docs/solutions/conventions/
module: release
problem_type: convention
component: documentation
severity: low
applies_when:
  - "On choisit le titre d'une PR"
  - "On relit une PR de release avant de la merger"
  - "On réécrit un changelog généré automatiquement"
tags:
  - changelog
  - release-notes
  - conventional-commits
  - release-please
---

# Un changelog ne liste que ce qui change pour un utilisateur

## Context

Généré à partir des commits, le changelog de l'opérateur faisait 293 lignes pour 14 versions et se lisait comme
un journal de commits. La revue du 2026-10-10 y a trouvé des doublons, des entrées d'outillage (hooks de suivi
des tokens listés comme « Features »), de la documentation interne, des unités d'implémentation (« U1 — CRD
types »), des corrections de bugs jamais publiés (retours de review sur du code pas encore sorti), des
« trigger rebuild », et aucune note de mise à jour.

## Guidance

Une entrée de changelog répond à une question : qu'est-ce qui change pour quelqu'un qui installe ou met à jour
l'opérateur ou ses charts ?

- **Y entrent** : comportement de l'opérateur, CRDs, charts, image.
- **N'y entrent pas** : outillage, CI, tests, documentation interne, refactorings, corrections apportées à du
  code qui n'avait pas encore été publié.
- **Une ligne par changement**, regroupée par fonctionnalité et non par commit.
- **Une section « Upgrade notes »** dès que la mise à jour demande une action : ordre d'installation des
  charts, ressource à recréer, autorisation à poser.
- **Une version sans effet pour l'utilisateur** porte « No user-facing changes ».

Ces règles sont tenues par trois réglages plutôt que par une relecture :

- le squash merge fait du titre de la PR l'entrée du changelog ;
- `feat` et `fix` sont réservés aux changements visibles par un utilisateur ; le reste se titre `chore`,
  `ci`, `test`, `docs` ou `refactor` ;
- `release-please-config.json` masque les sections Documentation et Refactoring.

## Why This Matters

Le renommage du groupe d'API en 0.1.1 et le retrait de deux CRDs du chart en 0.1.8 demandaient une action de
l'utilisateur, et figuraient au milieu d'entrées de documentation, sans rien qui les distingue. Un changelog
trop bruyant n'est plus lu, y compris quand il contient une information nécessaire.

## When to Apply

- Au moment de titrer une PR : le type choisi décide si elle produit une version et une entrée.
- À la relecture d'une PR de release : chaque ligne doit avoir un sens pour un utilisateur.
- `AGENTS.md` interdit d'éditer un `CHANGELOG.md` à la main, sauf réécriture décidée par le mainteneur.

## Examples

La version 0.1.14 comptait 57 lignes, dont onze pour ScalewaySecret. Réécrite :

```markdown
### Features

* **scaleway-secret:** add the `ScalewaySecret` CRD, which syncs one key of a Kubernetes Secret to Scaleway
  Secret Manager and pushes a new version when the Secret changes (#35)

### Upgrade notes

* The operator only reads a source Secret that opts in: it must carry the label
  `scaleway.mathieubodin.io/allow-operator-read: "true"` and the annotation
  `scaleway.mathieubodin.io/allowed-cr: "<namespace>/<name>"`.
```

Deux points de mécanique :

- release-please insère une nouvelle version avant le premier en-tête de version. Un paragraphe d'introduction
  placé sous `# Changelog` est donc conservé.
- Les notes des releases déjà publiées sur GitHub ne sont pas modifiées par la réécriture des fichiers.

## Related

- `docs/solutions/tooling-decisions/release-please-squash-merge-avoids-duplicate-changelog-entries.md`
- `CONTRIBUTING.md`, section « Soumettre une PR »
- Issue #157
