---
title: "release-please changelog-path est relatif au répertoire du package, pas au repo"
date: 2026-05-11
category: docs/solutions/integration-issues/
module: release_pipeline
problem_type: integration_issue
component: development_workflow
severity: high
symptoms:
  - "helm package échoue avec : Chart.yaml file is missing"
  - "Un répertoire charts/ fantôme apparaît dans le chart tree après un release-please run"
  - "release-please génère le tag et le CHANGELOG.md, mais helm package plante ensuite"
root_cause: config_error
resolution_type: config_change
tags:
  - release-please, helm, changelog, ci-cd, multi-package
---

# release-please changelog-path est relatif au répertoire du package, pas au repo

## Problem

Dans `release-please-config.json`, `changelog-path` est résolu relativement au répertoire de chaque package, pas à la racine du repo. Spécifier `"charts/scaleway-operator-crds/CHANGELOG.md"` pour un package dont le répertoire est `charts/scaleway-operator-crds` produit le chemin concaténé `charts/scaleway-operator-crds/charts/scaleway-operator-crds/CHANGELOG.md`. Helm interprète le préfixe `charts/` comme un sous-chart et échoue car il n'y a pas de `Chart.yaml` à cet emplacement.

## Symptoms

- `helm package charts/scaleway-operator-crds/` échoue avec :
  ```
  Error: Chart.yaml file is missing in charts/scaleway-operator-crds/charts/scaleway-operator-crds
  ```
- Un répertoire `charts/scaleway-operator-crds/charts/` est créé après un merge de PR release-please.
- `helm lint` passe (il ne valide pas les fichiers non-déclarés dans `charts/`), mais `helm package` échoue.

## What Didn't Work

Ajouter le chemin dans `.helmignore` — Helm se plaint avant même le packaging car il détecte un sous-répertoire `charts/` avec du contenu sans `Chart.yaml`.

## Solution

Dans `release-please-config.json`, utiliser un chemin relatif simple :

**Avant :**
```json
"charts/scaleway-operator-crds": {
  "release-type": "helm",
  "changelog-path": "charts/scaleway-operator-crds/CHANGELOG.md"
}
```

**Après :**
```json
"charts/scaleway-operator-crds": {
  "release-type": "helm",
  "changelog-path": "CHANGELOG.md"
}
```

Avec `"CHANGELOG.md"`, release-please écrit le changelog à `charts/scaleway-operator-crds/CHANGELOG.md`.

## Why This Works

La documentation release-please précise que `changelog-path` est résolu relativement à `package-path`. Le package étant ancré sur `charts/scaleway-operator-crds`, un nom de fichier simple `"CHANGELOG.md"` place le changelog exactement là où il faut.

## Prevention

- Pour tout package release-please dont `package-path` n'est pas la racine du repo, `changelog-path` doit être un nom de fichier seul (`"CHANGELOG.md"`), jamais un chemin complet depuis la racine.
- Ajouter `helm lint` **et** `helm package --dry-run` dans la CI avant de merger une PR touchant `release-please-config.json`.

## Related

- `release-please-config.json`
- `.github/workflows/release.yml`
- `docs/solutions/integration-issues/release-please-semver-docker-use-version-not-tag-name-2026-05-11.md`
