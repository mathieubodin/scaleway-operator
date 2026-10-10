---
title: "Mettre les PRs dans un Project GitHub sans doublons"
date: 2026-10-10
category: docs/solutions/tooling-decisions/
module: project-tracking
problem_type: tooling_decision
component: tooling
severity: low
applies_when:
  - "On veut qu'une PR affiche son projet, son milestone et ses champs"
  - "On ajoute des PRs à un Project qui ne contenait que des issues"
  - "On automatise un Project personnel par l'API"
tags:
  - github-project
  - pull-requests
  - insights
  - graphql
  - milestones
---

# Mettre les PRs dans un Project GitHub sans doublons

## Context

Les PRs n'affichaient ni milestone, ni label, ni projet : GitHub ne recopie rien de l'issue vers la PR qui la
ferme. Ajouter les PRs au Project règle l'affichage, mais chaque travail y apparaît alors deux fois, une
ligne pour l'issue et une pour la PR, dans les vues comme dans les graphiques.

## Guidance

**Séparer par filtre.** Les vues et les graphiques qui comptent le travail filtrent sur `is:issue`. Une vue
dédiée, filtrée sur `is:pr`, liste les PRs avec leur coût.

**Recopier les champs de l'issue sur la PR** (`scripts/sync-pr-metadata.sh`) : milestone, labels, Axis,
Priority, Effort. Aucun élément du Project ne reste incomplet, et les coûts portés par les PRs s'additionnent
par milestone ou par axe.

**Savoir ce que l'API permet avant de promettre une automatisation :**

| Élément | Par l'API | Détail |
| --- | --- | --- |
| Filtre d'une vue | oui | mutation `updateProjectV2View`, champ `filter` |
| Création d'une vue et de ses colonnes | oui | `createProjectV2View`, `configuration.visibleFieldIds` |
| Regroupement et somme d'une vue | non | interface web |
| Graphiques Insights | non | interface web, création comme filtre |
| Automatisations natives | lecture seule | `projectV2.workflows` donne le nom et l'état, pas la valeur réglée |

`createProjectV2View` n'accepte pas de filtre : il faut créer la vue, puis l'appeler avec `updateProjectV2View`.

## Why This Matters

Sans filtre, « Répartition par Status » et « Effort par Axis » comptent chaque travail deux fois dès la
première PR ajoutée. Le défaut est silencieux : les graphiques restent plausibles.

## When to Apply

- Avant d'ajouter des PRs à un Project : poser `is:issue` sur toutes les vues et tous les graphiques existants.
- À chaque nouvelle vue ou nouveau graphique : choisir explicitement `is:issue` ou `is:pr`.

## Examples

Poser le filtre sur toutes les vues :

```bash
gh api graphql -f query='query{ user(login:"OWNER"){ projectV2(number:2){ views(first:20){ nodes{ id name } } } } }' \
  --jq '.data.user.projectV2.views.nodes[].id' | while read -r id; do
  gh api graphql -f id="$id" \
    -f query='mutation($id:ID!){ updateProjectV2View(input:{viewId:$id, filter:"is:issue"}){ projectV2View{ name } } }'
done
```

Trois effets de bord observés :

- **Le compteur d'un milestone inclut les PRs.** M1 est passé de 26 à 55 éléments fermés après la reprise des
  anciennes PRs. Les graphiques filtrés sur `is:issue` ne sont pas touchés.
- **Insights sait tracer la somme d'un champ dans le temps.** Un graphique en courbe, filtré sur `is:pr`, avec
  la somme du champ `Tokens`, donne le coût cumulé. Chaque PR y est placée à sa date, pas à celle où la valeur
  a été inscrite.
- **Une PR mergée en squash laisse une branche que git croit non mergée.** `git branch -r --merged` ne la
  reconnaît pas ; vérifier par `gh pr list --state all --head <branche>`.

## Related

- `CONTRIBUTING.md`, section « Automatisations du Project »
- `docs/solutions/tooling-decisions/token-cost-per-pr-output-tokens.md`
- Issue #152
