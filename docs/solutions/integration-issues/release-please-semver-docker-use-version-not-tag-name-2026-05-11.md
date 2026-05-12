---
title: "release-please multi-package : utiliser outputs.version pour Docker, pas tag_name"
date: 2026-05-11
category: docs/solutions/integration-issues/
module: release_pipeline
problem_type: integration_issue
component: development_workflow
severity: high
symptoms:
  - "L'image Docker est taguée sha-xxxx uniquement — pas de tag semver ni de tag latest"
  - "docker/metadata-action reçoit scaleway-operator-v0.1.2 au lieu de 0.1.2"
  - "Le pattern type=semver ne produit aucun tag versionné"
root_cause: wrong_api
resolution_type: config_change
tags:
  - github-actions, docker, release-please, semver, multi-package, ci-cd
---

# release-please multi-package : utiliser outputs.version pour Docker, pas tag_name

## Problem

Dans un repo multi-package géré par release-please, `outputs.tag_name` contient le préfixe de composant : `scaleway-operator-v0.1.2`. Passé à `docker/metadata-action` avec `type=semver`, le pattern échoue à parser la chaîne (ce n'est pas un semver valide) — seul le tag `sha-xxxx` est généré. Les tags `latest` et versionnés sont absents.

## Symptoms

- L'image Docker ne porte que le tag `sha-<commit>` après une release.
- Le tag `latest` n'est pas mis à jour lors d'une release.
- L'image est inutilisable par les charts Helm qui référencent `tag: latest`.
- Le pipeline CI se termine sans erreur — le problème est silencieux.

## What Didn't Work

Stripper le préfixe avec une expression shell dans le champ `value:` de `docker/metadata-action` — les expressions GitHub Actions ne permettent pas les appels shell dans ce contexte.

## Solution

Utiliser `outputs.version` à la place de `outputs.tag_name`. Dans un repo multi-package, `version` contient le semver nu (`0.1.2`) sans le préfixe de composant.

**Avant :**

```yaml
tags: |
  type=semver,pattern={{version}},value=${{ needs.release-please.outputs.tag_name }}
  type=semver,pattern={{major}}.{{minor}},value=${{ needs.release-please.outputs.tag_name }}
```

**Après :**

```yaml
tags: |
  type=semver,pattern={{version}},value=${{ needs.release-please.outputs.version }}
  type=semver,pattern={{major}}.{{minor}},value=${{ needs.release-please.outputs.version }}
```

| Output release-please | Valeur exemple              | Usage correct                    |
|-----------------------|-----------------------------|----------------------------------|
| `tag_name`            | `scaleway-operator-v0.1.2`  | Référence au tag Git             |
| `version`             | `0.1.2`                     | Semver à passer aux outils (Docker, Helm) |

## Why This Works

`tag_name` est conçu pour le tag Git en format `{component}-v{version}`. `version` est le semver nu, prévu pour être consommé par des outils qui attendent un semver standard. `docker/metadata-action` avec `type=semver` requiert un semver valide dans `value`.

## Prevention

- Dans tout workflow consommant les outputs de release-please pour tagger une image Docker : toujours utiliser `outputs.version`, jamais `outputs.tag_name`.
- Vérifier après chaque release que l'image porte les trois tags attendus (`x.y.z`, `x.y`, `latest`).

## Related

- `.github/workflows/release.yml`
- `docs/solutions/integration-issues/release-please-changelog-path-relative-to-package-dir-2026-05-11.md`
