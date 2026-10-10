---
title: "Mesurer le coût d'une PR en tokens produits, porté par la PR"
date: 2026-10-10
category: docs/solutions/tooling-decisions/
module: project-tracking
problem_type: tooling_decision
component: tooling
severity: medium
applies_when:
  - "On veut comparer le coût IA de deux développements"
  - "On lit l'usage d'une session Claude Code dans son journal JSONL"
  - "On choisit où inscrire une mesure dans un Project GitHub qui contient issues et PRs"
tags:
  - tokens
  - cost-tracking
  - claude-code
  - prompt-cache
  - github-project
  - hooks
---

# Mesurer le coût d'une PR en tokens produits, porté par la PR

## Context

Le champ `Tokens` du Project GitHub devait donner le coût IA de chaque développement. Trois choix
successifs se sont révélés faux en deux jours :

- **Compter tout sauf les relectures de cache.** Le journal d'une session distingue quatre compteurs :
  `input_tokens`, `output_tokens`, `cache_creation_input_tokens` et `cache_read_input_tokens`. Écarter
  seulement les relectures semblait raisonnable. Sur une session de deux jours, ce total valait
  2,55 millions, dont 1,42 million en quatre fois seulement, entre 330 000 et 380 000 à chaque fois.
- **Inscrire le coût sur l'issue.** Une PR qui fermait trois issues inscrivait trois fois son coût, une
  issue traitée par deux PRs ne gardait que le second relevé, et une PR sans issue n'avait nulle part
  où l'inscrire.
- **Compter sur l'agent pour le relever.** La règle était écrite dans `AGENTS.md`. Une seule issue sur
  sept avait sa valeur.

## Guidance

**Le chiffre : `output_tokens` seul**, sous-agents compris. C'est le texte produit par l'assistant.

Les pics de `cache_creation_input_tokens` tombent à chaque reprise d'une session après une pause : le
cache de prompt expire au bout d'une heure, et toute la conversation est remise en cache d'un coup. Ce
compteur mesure donc les pauses du mainteneur et la longueur de la conversation, pas le travail de la PR.

```bash
# Tokens produits depuis un instant donné, journal principal et sous-agents
cat "$transcript" "${transcript%.jsonl}"/subagents/*.jsonl | jq -c --arg since "$since" '
    select(.type == "assistant" and .message.usage != null and .timestamp > $since)
    | {id: .message.id, tokens: (.message.usage.output_tokens // 0)}' |
    jq -s 'unique_by(.id) | map(.tokens) | add // 0'
```

Le `unique_by(.id)` est indispensable : un même message apparaît sur plusieurs lignes du journal, une
par bloc de contenu, chacune avec le même `usage`.

**Le porteur : l'élément du Project qui représente la PR**, pas l'issue. Une PR, un chiffre, jamais
dupliqué ni écrasé. Comme `scripts/sync-pr-metadata.sh` recopie sur la PR le milestone et l'Axis de son
issue, les coûts s'additionnent par milestone ou par axe sans passer par les issues.

**Le déclencheur : un hook, pas une règle.** `scripts/report-tokens.sh` est lancé par un hook
`PostToolUse` de Claude Code après chaque `gh pr create` et `git push`. La dernière valeur l'emporte,
donc le champ est à jour au moment du merge, qui se fait dans l'interface web et ne déclenche rien en local.

## Why This Matters

Avec les tokens de cache, la même PR coûtait 80 000 ou 450 000 selon que le mainteneur avait fait une
pause au milieu. Le chiffre n'était comparable ni d'une PR à l'autre ni d'un jour à l'autre, ce qui
retirait tout intérêt au suivi.

Les tokens produits sous-estiment le travail de lecture, une review par exemple. C'est un biais connu
et stable, préférable à un bruit cinq fois supérieur au signal.

## When to Apply

- Pour toute mesure tirée du journal d'une session : vérifier d'abord la répartition entre les quatre
  compteurs avant d'en additionner plusieurs.
- Pour toute mesure inscrite dans un Project : la porter par l'objet réellement mesuré. Ici le travail
  mesuré est celui d'une PR.
- Pour toute règle qu'un agent doit appliquer à chaque PR : préférer un hook qui l'exécute et un contrôle
  de CI qui signale son absence (`pr-metadata.yml`).

## Examples

Répartition observée sur la session du 9 et du 10 octobre 2026 :

| Compteur | Tokens |
| --- | --- |
| `output_tokens` | 237 173 |
| `input_tokens` | 808 |
| `cache_creation_input_tokens` | 2 311 111 |
| `cache_read_input_tokens` | 85 628 752 |

Les quatre plus grosses mises en cache, toutes à une reprise de session :

| Instant (UTC) | `cache_creation_input_tokens` |
| --- | --- |
| 2026-10-09 10:17 | 333 640 |
| 2026-10-10 07:48 | 342 926 |
| 2026-10-10 10:12 | 362 703 |
| 2026-10-10 13:02 | 381 468 |

Limite de l'attribution : une PR reçoit les tokens produits depuis le dernier relevé de la PR
précédente dans la même session. Une discussion sans rapport, tenue entre deux PRs, est comptée sur la
suivante.

## Related

- `CONTRIBUTING.md`, section « Convention `/cost N` »
- `scripts/report-tokens.sh`, `scripts/sync-pr-metadata.sh`, `.github/workflows/pr-metadata.yml`
- `docs/solutions/integration-issues/git-trailer-block-poisoning-closes-footer-interpret-trailers-2026-06-11.md` :
  le mécanisme précédent, par trailers de commit, abandonné
- Issues #45, #149, #152 et #154
