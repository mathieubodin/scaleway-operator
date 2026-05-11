---
title: Lire la section complète avant d'éditer un document structuré
date: 2026-05-12
category: docs/solutions/conventions/
module: documentation
problem_type: convention
component: documentation
severity: low
applies_when:
  - Édition d'une section de documentation avec plusieurs sous-sections interdépendantes
  - Sections ayant une liste introductive et des sous-sections numérotées correspondantes
  - Refactoring ou restructuration de README, CLAUDE.md, plans, ou specs
tags:
  - documentation
  - readme
  - editing
  - structural-consistency
---

# Lire la section complète avant d'éditer un document structuré

## Context

Lors de l'implémentation des améliorations de la section Installation du README, les edits ont été appliqués séquentiellement par requirement sans relecture préalable de la section entière. L'intro a été modifiée en "Quatre étapes" avec une liste ajoutant "1. Vérifier les prérequis" — mais les sous-sections numérotées commençaient à `### 1. Installer les CRDs`. L'incohérence n'a été détectée qu'en relecture post-implémentation.

Le bug : la liste d'intro et les sous-sections numérotées sont deux représentations du même séquençage — modifier l'une sans avoir la vue d'ensemble de l'autre produit des incohérences silencieuses.

## Guidance

Avant de commencer le premier edit sur une section de documentation avec des sous-sections interdépendantes, lire la section complète une fois pour cartographier :

1. Les éléments structurels (listes d'intro, sous-sections, références croisées internes)
2. Les dépendances implicites entre ces éléments (liste intro ↔ sous-sections numérotées, texte introductif ↔ sous-section Prérequis)
3. Le périmètre exact de chaque modification planifiée et ses effets de bord

## Why This Matters

Les sections de documentation bien structurées ont des invariants implicites : une liste d'intro décrit exactement les sous-sections qui suivent, les références ("voir étape 4") pointent vers des sous-sections existantes, les comptes ("Quatre étapes") correspondent au nombre de sous-sections numérotées. Ces invariants ne sont visibles qu'avec la vue d'ensemble — pas en lisant un seul paragraphe pour un edit isolé.

Sans cette relecture préalable, des incohérences comme une liste à 4 items décrivant 3 étapes (ou inversement) passent inaperçues jusqu'à la relecture finale.

## When to Apply

- Toute section avec une liste d'intro suivie de sous-sections numérotées (`### 1.`, `### 2.`…)
- Toute section avec des références internes (`voir Prérequis ci-dessus`, `voir étape 3`)
- Refactoring d'une section existante (ajout, suppression, réorganisation de sous-sections)

Ne s'applique pas aux edits ponctuels d'une phrase isolée sans dépendance structurelle.

## Examples

### Situation typique (README Installation)

```
## Installation

Trois étapes :              ← liste d'intro
1. CRDs
2. Credentials
3. Opérateur

### 1. Installer les CRDs    ← sous-sections numérotées
### 2. Configurer...
### 3. Déployer...
### 4. Vérifier l'installation  ← déjà présente !
```

**Sans relecture préalable :** ajout de "1. Vérifier les prérequis" en tête de liste → la liste dit 4 étapes dont "1. prérequis" mais les sous-sections numérotées commencent à "1. CRDs" → incohérence.

**Avec relecture préalable :** on voit que `### 4.` existe déjà → la liste doit garder les 3 étapes existantes et ajouter "4. Vérifier l'installation", pas substituer l'étape 1.

## Related

- Convention connexe : `docs/solutions/conventions/bash-multiline-comment-breaks-continuation-2026-05-12.md`
- Plan source : `docs/plans/2026-05-12-001-docs-readme-installation-improvements-plan.md`
