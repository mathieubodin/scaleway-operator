---
title: "release-please : le squash merge évite les entrées de changelog en double"
date: 2026-10-10
category: docs/solutions/tooling-decisions/
module: release
problem_type: tooling_decision
component: tooling
severity: medium
applies_when:
  - "Le changelog généré liste deux fois le même changement"
  - "On choisit le mode de merge d'un dépôt publié par release-please"
  - "On veut corriger les notes d'une release avant sa publication"
  - "La PR de release est en conflit avec main après un merge"
tags:
  - release-please
  - changelog
  - squash-merge
  - merge-commit
  - conventional-commits
---

# release-please : le squash merge évite les entrées de changelog en double

## Context

À la release 0.1.15, chaque correction apparaissait deux fois dans le changelog, avec deux SHA différents.
Le défaut existait déjà en 0.1.14. Trois pistes ont été essayées avant de trouver la cause.

- **Le message du commit de merge.** Le dépôt était en `merge_commit_title: MERGE_MESSAGE` et
  `merge_commit_message: PR_TITLE`. GitHub n'accepte que trois combinaisons : `PR_TITLE` + `PR_BODY`,
  `PR_TITLE` + `BLANK`, `MERGE_MESSAGE` + `PR_TITLE`. Toutes placent le titre de la PR dans le commit de merge.
- **`BEGIN_COMMIT_OVERRIDE` dans la description de la PR**, pour neutraliser le commit de merge. La surcharge
  s'applique à tous les commits associés à la PR, pas au seul merge : les vraies entrées ont disparu aussi,
  et release-please a répondu « No user facing commits found ».
- **`exclude-paths`**, pour écarter les commits d'outillage. Il ne filtre que des répertoires, et seulement
  les commits dont tous les fichiers sont exclus. Un commit qui touche aussi `Makefile` ou `CONTRIBUTING.md`
  reste compté.

## Guidance

**Merger en squash.** Avec un merge commit, une PR laisse sur `main` son commit et le commit de merge, qui
portent le même intitulé conventionnel : release-please lit les deux. Avec un squash, il reste un commit par
PR, dont le message est le titre de la PR.

Réglages du dépôt :

```bash
gh api -X PATCH repos/OWNER/REPO \
  -F allow_squash_merge=true -F allow_merge_commit=false -F allow_rebase_merge=false \
  -f squash_merge_commit_title=PR_TITLE -f squash_merge_commit_message=BLANK
```

`squash_merge_commit_message` doit valoir `BLANK` : la valeur par défaut recopie la liste des commits de la PR
dans le corps, que release-please lirait comme autant de commits conventionnels supplémentaires.

**Le titre de la PR devient la seule source des versions et du changelog.** `feat` et `fix` sont réservés à ce
qui change pour un utilisateur ; l'outillage se titre `chore`, `ci`, `test` ou `docs`. Le workflow
`pr-metadata.yml` vérifie le format du titre.

**Corriger les notes d'une release en cours.** Les commits déjà sur `main` ne se corrigent pas. Il faut réécrire
la section dans le `CHANGELOG.md` de la branche de release et dans la description de la PR : c'est la
description, et non le fichier, que `release-please github-release` reprend comme notes de la release GitHub.

## Why This Matters

Un changelog en double, mêlé d'entrées d'outillage, ne se lit plus. Et la tentative de surcharge a retiré
toute la release sans erreur : la PR existante est restée affichée avec son ancien contenu, ce qui masquait
que plus rien n'aurait été publié.

## When to Apply

- Dès qu'un dépôt est publié par release-please : choisir le squash avant la première release.
- Avant d'utiliser `BEGIN_COMMIT_OVERRIDE` : vérifier son effet par un `release-pr --dry-run`.
- Quand `main` avance sans changer les notes de release : release-please répond « remained the same » et
  ne reconstruit pas sa branche, qui finit en conflit. `scripts/release-prepare.sh` le détecte par
  `git merge-base --is-ancestor origin/main origin/<branche>` et force la reconstruction en modifiant la
  description de la PR.

## Examples

Détection du problème, sans rien publier :

```bash
npx release-please@17.11.2 release-pr --dry-run --debug \
  --token="$(gh auth token)" --repo-url=OWNER/REPO \
  --config-file=release-please-config.json --manifest-file=.release-please-manifest.json
```

La sortie indique, par composant, le nombre de commits considérés et « Would open N pull requests ».

Autre piège de la même session : chercher la PR de release par son label juste après sa création échoue,
la recherche GitHub ayant du retard. La lister par sa branche (`gh pr list --head <branche>`) est immédiat.

## Related

- `docs/solutions/integration-issues/release-please-semver-docker-use-version-not-tag-name-2026-05-11.md`
- `docs/solutions/tooling-decisions/release-please-extra-files-generic-updater-blind-spots-2026-05-12.md`
- `CONTRIBUTING.md`, sections « Publier une release » et « Soumettre une PR »
- Issues #157 et #159
