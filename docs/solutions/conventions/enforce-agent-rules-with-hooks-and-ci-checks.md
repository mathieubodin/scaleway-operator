---
title: "Une règle d'agent se fait exécuter par un hook et vérifier par la CI"
date: 2026-10-10
category: docs/solutions/conventions/
module: agent-workflow
problem_type: convention
component: development_workflow
severity: medium
applies_when:
  - "Une règle d'AGENTS.md doit être appliquée à chaque PR"
  - "Le mainteneur doit rappeler à l'agent une étape qu'il a oubliée"
  - "On ajoute une étape répétitive au cycle d'une PR"
tags:
  - claude-code
  - hooks
  - github-actions
  - agents-md
  - automation
---

# Une règle d'agent se fait exécuter par un hook et vérifier par la CI

## Context

`AGENTS.md` demandait à l'agent de renseigner le coût en tokens en fin de PR. En deux jours, une seule issue
sur sept avait sa valeur. La même session a montré le même oubli sur les métadonnées des PRs : ni milestone,
ni label, ni personne assignée, ni projet. À chaque fois, c'est le mainteneur qui l'a remarqué en relisant.

Une règle écrite dépend de l'attention de celui qui l'applique. Elle ne tient pas sur une étape répétitive
placée en fin de tâche.

## Guidance

Pour chaque règle répétitive, répartir le travail entre trois mécanismes :

| Mécanisme | Rôle | Limite |
| --- | --- | --- |
| Hook Claude Code | exécute l'action sans décision de l'agent | sessions Claude Code du poste seulement ; un échec passe inaperçu |
| Contrôle de CI | rend l'absence visible sur la PR, quel que soit l'auteur | ne voit que ce que l'événement de la PR contient |
| Règle dans `AGENTS.md` | explique l'intention et le repli manuel | ne garantit rien |

Dans ce dépôt :

- le hook `PostToolUse` déclaré dans `.claude/settings.json` lance `scripts/report-tokens.sh --hook` après
  chaque commande Bash ; le script n'agit que sur `gh pr create` et `git push`, et lance
  `scripts/sync-pr-metadata.sh` à l'ouverture d'une PR ;
- le workflow `pr-metadata.yml` vérifie le titre, l'issue référencée, le milestone, le label et l'assignation ;
- `AGENTS.md` renvoie aux deux et donne les commandes de repli (`make sync-pr-metadata`, `make report-tokens`).

Trois détails de mise en œuvre :

- **Choisir un événement local certain de se produire.** Le merge se fait dans l'interface web et ne
  déclenche rien sur le poste. L'ouverture et la mise à jour de la PR, elles, passent par le shell.
- **Un hook ne doit jamais faire échouer la commande qui l'a déclenché** : `trap 'exit 0' ERR` en mode hook.
- **Le contrôle de CI doit se relancer quand les métadonnées changent** : types `labeled`, `milestoned`,
  `assigned`, `edited` de l'événement `pull_request`, dans un workflow séparé pour ne pas relancer les tests.

## Why This Matters

Sans contrôle, l'oubli n'est découvert qu'à la relecture, et le mainteneur devient le garde-fou. Le hook seul
ne suffit pas non plus : il n'est actif qu'après un redémarrage de session, et il ne couvre ni une PR ouverte
à la main ni un autre outil.

## When to Apply

- Quand une étape doit suivre chaque PR, chaque commit ou chaque release.
- Quand le mainteneur a dû rappeler la même étape deux fois.
- Pas pour une règle de jugement, qui ne se réduit pas à une commande ni à une vérification.

## Examples

Ce que la CI ne peut pas vérifier ici : la présence de la PR dans le Project et son champ `Tokens`. Lire un
Project personnel demande un token personnel, que le dépôt n'héberge plus. Ces deux points ne sont couverts
que par le hook. Un hook `Stop`, qui interroge GitHub avant de rendre la main, pourrait les couvrir au prix
d'un appel réseau à chaque fin de tour.

Effet visible du couple hook et contrôle : à l'ouverture, `pr-metadata` passe au rouge, puis au vert quelques
secondes plus tard, une fois les métadonnées recopiées.

Limite de la protection du fichier de réglages : `.claude/settings.json` interdit à l'agent de le modifier
(`deny` sur `Edit(*.claude/settings*)`). Le hook est donc ajouté par le mainteneur, l'agent ne faisant que
committer le changement.

## Related

- `docs/solutions/tooling-decisions/token-cost-per-pr-output-tokens.md`
- `CONTRIBUTING.md`, sections « Automatisations du Project » et « Convention `/cost N` »
- Issues #149, #152 et #154
