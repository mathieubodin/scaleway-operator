---
title: "release-please extra-files: le generic updater est aveugle et glouton"
date: 2026-05-12
category: docs/solutions/tooling-decisions/
module: release-please
problem_type: tooling_decision
component: tooling
severity: medium
applies_when:
  - Configuration de extra-files dans release-please-config.json pour un package helm
  - Maintien automatique de versions dans un fichier partagé comme README.md
  - Plusieurs packages helm ayant des versions normalement synchrones
tags:
  - release-please
  - extra-files
  - helm
  - generic-updater
  - versioning
  - readme
---

# release-please extra-files : le generic updater est aveugle et glouton

## Context

> **Note préventive** : `extra-files` n'est pas configuré dans l'actuel `release-please-config.json`. Ce document documente les angles morts à connaître **avant** d'ajouter cette configuration.

Si `extra-files: ["README.md"]` était ajouté sous le package `charts/scaleway-operator` pour maintenir automatiquement les versions helm dans le README, une analyse des angles morts révèle trois contraintes non documentées officiellement.

Configuration hypothétique :

```json
"charts/scaleway-operator": {
  "release-type": "helm",
  "extra-files": ["README.md"],
  ...
}
```

README.md cible contenant deux commandes helm :

```bash
helm upgrade scaleway-operator-crds ... --version 0.1.6 \   # crds
helm upgrade scaleway-operator     ... --version 0.1.6 \   # operator
```

## Guidance

### Contrainte 1 — Le generic updater remplace TOUTES les occurrences

Le generic updater de release-please cherche la chaîne de version courante (ex. `0.1.6`) dans l'intégralité du fichier et remplace **chaque occurrence** par la nouvelle version. Il ne cible pas une ligne spécifique. Les commentaires `# crds` / `# operator` sont transparents pour lui — ils ne servent qu'à la lisibilité humaine.

**Conséquence :** si `0.1.6` apparaît ailleurs dans README.md (prose, exemples de changelog, tutoriel), ces occurrences seront aussi remplacées.

**Mitigation :** vérifier à chaque release que la chaîne de version ne se trouve que dans les lignes helm cibles. Pour du ciblage strict, utiliser les marqueurs explicites :

```markdown
<!-- x-release-please-start-version -->0.1.6<!-- x-release-please-end-version -->
```

### Contrainte 2 — La divergence de versions entre packages casse le README

Si `charts/scaleway-operator` bumpe (0.1.6 → 0.1.7) mais que `charts/scaleway-operator-crds` ne bumpe pas (reste à 0.1.6) :

- Le generic updater remplace `0.1.6` → `0.1.7` dans README.md
- Les **deux** commandes helm affichent `--version 0.1.7`
- Mais `scaleway-operator-crds` est toujours publié à `0.1.6` sur GHCR

Le README devient faux pour la commande CRDs. L'erreur est silencieuse — aucun CI ne détecte que la version dans le README ne correspond pas au chart publié.

**Mitigation pour le test et les releases normales :** toujours bumper les deux charts dans le même commit, en touchant les deux `Chart.yaml` dans un seul `fix:` ou `feat:`. Cela garantit que les deux packages sont inclus dans le même Release PR et bumpent à la même version.

### Contrainte 3 — Le commit déclencheur doit toucher les deux Chart.yaml

Release-please manifest mode attribue les bumps de version par package selon les fichiers modifiés par chaque commit. Un commit qui touche uniquement `charts/scaleway-operator/Chart.yaml` ne bumpe que ce package — pas `charts/scaleway-operator-crds`.

Pour déclencher un bump synchronisé des deux charts, le commit doit modifier des fichiers dans les **deux** répertoires de charts.

Exemple de commit correct pour l'appVersion fix :

```bash
git add charts/scaleway-operator/Chart.yaml charts/scaleway-operator-crds/Chart.yaml
git commit -m "fix(charts): align appVersion with deployed binary version 0.1.6"
```

## Why This Matters

Ces trois contraintes interagissent : un commit mal ciblé (une seule Chart.yaml) déclenche une divergence de versions, et le generic updater la propage silencieusement dans README.md. La détection ne se fait qu'en vérifiant manuellement le diff du Release PR avant de le merger.

Sans cette connaissance, un agent ou développeur configurant `extra-files` peut croire que les `# crds`/`# operator` comments protègent contre la divergence — ils ne le font pas.

## When to Apply

- Avant tout test de la mécanique `extra-files` : s'assurer que le commit déclencheur touche les deux Chart.yaml
- Avant de merger un Release PR : vérifier dans le diff que les versions README correspondent aux versions des Chart.yaml inclus
- Si les deux charts commencent à bumper indépendamment : migrer vers les marqueurs `x-release-please-start-version` / `x-release-please-end-version` pour un ciblage ligne par ligne

## Examples

### Commit déclencheur incorrect (cause la divergence)

```bash
# Ne touche qu'un chart → operator bumpe, crds reste à 0.1.6 → README incorrect
git add charts/scaleway-operator/Chart.yaml
git commit -m "fix(charts): fix appVersion in operator chart"
```

### Commit déclencheur correct (bump synchronisé)

```bash
# Touche les deux charts → les deux bumpent ensemble → README correct
git add charts/scaleway-operator/Chart.yaml charts/scaleway-operator-crds/Chart.yaml
git commit -m "fix(charts): align appVersion with deployed binary version 0.1.6"
```

### Critères de validation d'un Release PR extra-files

Après merge du commit déclencheur, inspecter le Release PR avec :

```bash
gh pr view <PR_NUMBER> --json files | jq '[.files[].path]'
```

| Critère | Attendu si correct | Attendu si raté |
| --- | --- | --- |
| `README.md` dans les fichiers | Présent | Absent (path résolu comme package-relatif) |
| Diff `--version` lignes | `0.1.6` → `0.1.7` sur les 2 lignes | 0 lignes changées (pattern non trouvé) |
| `.release-please-manifest.json` | Les deux charts à `0.1.7` | Un seul chart à `0.1.7` (divergence) |

## Related

- `docs/solutions/integration-issues/release-please-changelog-path-relative-to-package-dir-2026-05-11.md` — les chemins `changelog-path` sont package-relatifs (comportement différent de `extra-files`)
- `docs/solutions/integration-issues/release-please-semver-docker-use-version-not-tag-name-2026-05-11.md` — utiliser `outputs.version` (semver nu) et non `outputs.tag_name` (préfixé)
- Plan source : `docs/plans/2026-05-12-001-docs-readme-installation-improvements-plan.md`
