---
title: "Trailers git non parseables quand un footer GitHub sans deux-points partage le bloc final"
date: 2026-06-11
category: docs/solutions/integration-issues/
module: ops
problem_type: integration_issue
component: tooling
symptoms:
  - "`git log --format='%(trailers:key=Claude-Tokens-Delta,valueonly)'` retourne vide pour certains commits pourtant porteurs de trailers"
  - "L'agrégation de tokens sous-compte silencieusement (3300 au lieu de 6300 mesuré en repo de test)"
  - "`git interpret-trailers --parse` ne retourne rien sur le message concerné"
root_cause: wrong_api
resolution_type: code_fix
severity: high
tags: [git-hooks, prepare-commit-msg, git-trailers, interpret-trailers, token-tracking, claude-code-hooks]
---

# Trailers git non parseables quand un footer GitHub sans deux-points partage le bloc final

## Problem

Le hook `prepare-commit-msg` du token tracking (PR [#108](https://github.com/mathieubodin/scaleway-operator/pull/108))
injectait manuellement les trailers `Claude-*` dans le message de commit. Dès que le message contenait un footer GitHub
sans deux-points (`Closes #7`) dans le même paragraphe final, git refusait de parser **tout** le bloc de trailers —
y compris les trailers `Claude-*` valides — et les requêtes d'agrégation sous-comptaient silencieusement.

## Symptoms

- `git log --format='%(trailers:key=Claude-Tokens-Delta,valueonly)'` vide pour certains commits qui affichent pourtant les trailers dans `git log --format=%B`.
- La somme agrégée des deltas est inférieure au total réellement consommé, sans aucune erreur visible.
- `git interpret-trailers --parse < message` ne retourne rien.

## What Didn't Work

- **Injection manuelle « avant la ligne `Co-Authored-By` »** (boucle `while read` réécrivant le fichier message) :
  passait les cas simples (message une ligne, footers conventional dans un paragraphe séparé), mais échouait sur la
  combinaison `Closes #N` + `Co-Authored-By` dans le même paragraphe — très courante puisque l'assistant termine ses
  messages de commit par `Co-Authored-By` et que GitHub recommande `Closes #N` pour lier les issues.
- **Garde anti-amend `[ "$2" = "commit" ] && [ -n "$3" ]`** : ne couvre pas `git commit --amend -m "..."`, car git
  passe alors `source=message`, indistinguable d'un commit normal. Aucune détection fiable n'existe dans
  `prepare-commit-msg` — documenté comme limitation dans CONTRIBUTING.md (préférer `--amend` sans `-m`, sinon le
  delta du commit remplacé est perdu).
- **Pièges annexes dans les hooks Claude Code** découverts au passage : garde `stop_hook_active` inversée dans le hook
  Stop (le sens correct est de sortir quand la valeur est `true` — continuation déclenchée par un hook Stop — sinon le
  hook ne compte jamais rien dans le cas nominal), et baseline non remise à zéro au changement de session dans le hook
  SessionStart (un commit avant le premier event Stop de la nouvelle session produisait un delta faux).

## Solution

Remplacer la réécriture manuelle du message par l'outil canonique `git interpret-trailers`.

Avant (extrait de la boucle manuelle) :

```bash
while IFS= read -r line || [[ -n "$line" ]]; do
    if [[ "$line" =~ ^Co-Authored-By: ]] && [[ $injected -eq 0 ]]; then
        printf "Claude-Session: %s\n..." >> "$tmpout"
        injected=1
    fi
    printf "%s\n" "$line" >> "$tmpout"
done < "$1"
```

Après ([.githooks/prepare-commit-msg](../../../.githooks/prepare-commit-msg)) :

```bash
git interpret-trailers --in-place --if-exists replace \
    --trailer "Claude-Session: $session_short" \
    --trailer "Claude-Tokens-Delta: $delta" \
    --trailer "Claude-Tokens-Total: $tokens_session" \
    "$1"
```

`--if-exists replace` rend de plus le hook idempotent si le message porte déjà des trailers `Claude-*`.

## Why This Works

Git ne reconnaît le dernier paragraphe d'un message comme bloc de trailers que si **toutes** ses lignes sont des
trailers valides (`token: valeur`), ou s'il contient un trailer git-generated (`Signed-off-by`). La syntaxe GitHub
`Closes #7` utilise le séparateur `#` et non `:` — ce n'est pas un trailer valide au sens git, et sa présence
invalide le bloc entier : c'est tout ou rien, pas ligne à ligne.

`git interpret-trailers` applique exactement ces règles au moment de l'injection : si le paragraphe final est un bloc
de trailers valide, il y ajoute les nouveaux trailers ; sinon il crée un **bloc séparé** après une ligne vide. Dans
les deux cas, le bloc final est pur et toujours parseable par `%(trailers:...)`.

## Prevention

- **Ne jamais réécrire un message de commit à la main quand git fournit l'outil canonique** : `interpret-trailers`
  encode des règles de bloc non triviales qu'une manipulation textuelle ne reproduit pas.
- **Tester les hooks git dans un repo jetable avec des cas adversariaux**, pas seulement le cas nominal :

```bash
git init /tmp/hook-test && cd /tmp/hook-test
ln -s <repo>/.githooks/prepare-commit-msg .git/hooks/prepare-commit-msg
# fabriquer les fichiers d'état, puis rejouer la suite :
# 1. message une ligne ; 2. body + footers conventional (BREAKING CHANGE:, Closes #42) ;
# 3. Closes #7 + Co-Authored-By dans le même paragraphe ; 4. git commit --amend sans -m
```

- **Vérifier le parsing, pas l'affichage** : `git log --format=%B` montre les trailers même quand git ne les parse
  pas. Le test qui compte est `git log --format='%(trailers:key=...,valueonly)'` ou `git interpret-trailers --parse`.
- **Toute requête d'agrégation doit être validée contre un total connu** (ici : somme attendue 25 000 dans le repo de
  test) — un sous-comptage silencieux est invisible autrement.

## Related Issues

- PR [#108](https://github.com/mathieubodin/scaleway-operator/pull/108) — feature token tracking (fix inclus)
- Issue [#106](https://github.com/mathieubodin/scaleway-operator/issues/106) — issue parente de la feature
- Issue [#45](https://github.com/mathieubodin/scaleway-operator/issues/45) — évaluation du tracking automatique des tokens
- [CONTRIBUTING.md](../../../CONTRIBUTING.md) — section « Token tracking git » (activation, requêtes, limitation `--amend -m`)
