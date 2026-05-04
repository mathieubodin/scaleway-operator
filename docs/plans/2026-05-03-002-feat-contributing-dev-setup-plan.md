---
title: "feat: Add dev environment setup to CONTRIBUTING.md"
type: feat
status: active
date: 2026-05-03
---

# feat: Add dev environment setup to CONTRIBUTING.md

## Overview

Le `CONTRIBUTING.md` existant décrit le processus de contribution (PR, style, tests) mais ne dit pas comment installer Rust ni les outils de développement. Un contributeur qui clone le dépôt ne peut pas exécuter `make check` ou `make coverage` sans savoir quoi installer au préalable.

## Problem Frame

`make coverage` échoue avec `cargo: Aucun fichier ou dossier de ce nom` sur une machine sans Rust installé. Aucun fichier du projet ne décrit l'installation de la toolchain Rust en contexte développeur (SETUP.md couvre le déploiement Kubernetes, pas le build local).

## Requirements Trace

- R1. Un développeur qui clone le dépôt peut trouver toutes les étapes d'installation dans `CONTRIBUTING.md`.
- R2. Les prérequis obligatoires sont distincts des prérequis optionnels (couverture, kubectl).
- R3. Chaque commande de développement documentée dans `CLAUDE.md` et `Makefile` est accessible depuis `CONTRIBUTING.md`.

## Scope Boundaries

- Ne pas dupliquer SETUP.md (déploiement Kubernetes en production).
- Ne pas documenter l'obtention d'un token Scaleway pour les tests d'intégration réels (hors scope dev local).
- Ne pas modifier le processus de contribution existant (bugs, PRs, style).

## Context & Research

### Relevant Code and Patterns

- `CONTRIBUTING.md` — section "🧪 Tests" existante à compléter, section "Prérequis de développement" à ajouter avant.
- `CLAUDE.md` — liste canonique des commandes (`cargo build`, `cargo test`, `make check`, `make coverage`).
- `Makefile` — cibles `test`, `coverage`, `coverage-open`, `check`, `fmt`, `clippy` et leur aide intégrée.
- `Cargo.toml` — `edition = "2021"`, dépendances : `kube = "0.90"`, `mockito = "1"` (dev).

### Institutional Learnings

- `rustup component add llvm-tools` et `cargo install cargo-llvm-cov` sont requis pour `make coverage` (ajoutés dans Makefile:help mais nulle part en prose).
- La version minimale de Rust n'est pas fixée explicitement — utiliser "stable récent" (1.75+ pour edition 2021 + kube 0.90).

## Key Technical Decisions

- **Intégrer dans CONTRIBUTING.md, pas créer un fichier séparé** : le fichier existe déjà et est le point d'entrée attendu pour un contributeur GitHub. Ajouter un `DEV.md` séparé créerait une fragmentation.
- **Section "Environnement de développement" en tête de fichier** : avant le processus de contribution — un contributeur doit pouvoir builder avant de soumettre une PR.
- **Séparer "requis" et "optionnel"** : Rust/Cargo sont requis. kubectl/docker/cargo-llvm-cov sont optionnels selon l'activité.

## Implementation Units

- U1. **Ajouter la section "Environnement de développement" dans CONTRIBUTING.md**

**Goal:** Permettre à un contributeur de passer de zéro à `cargo test` en suivant les instructions du fichier.

**Requirements:** R1, R2, R3

**Dependencies:** Aucune

**Files:**

- Modify: `CONTRIBUTING.md`

**Approach:**

Insérer une section `## 🛠️ Environnement de développement` en tête du fichier (avant "Signaler un bug"), structurée ainsi :

1. **Prérequis obligatoires** — Rust via rustup (https://rustup.rs), commande de vérification.
2. **Vérification de l'installation** — `cargo --version`, `rustc --version`.
3. **Commandes de développement** — tableau récapitulatif des cibles Makefile + commandes cargo directes.
4. **Prérequis optionnels** — sous-section avec kubectl (pour `make deploy`), docker (pour `make docker-build`), cargo-llvm-cov + llvm-tools (pour `make coverage`).
5. **Variables d'environnement** — liste des variables requises à l'exécution de l'opérateur (pas nécessaires pour `cargo test`).

La section "🧪 Tests" existante peut rester telle quelle ou référencer le Makefile, mais ne doit pas être dupliquée.

**Patterns to follow:**

- Ton et style de l'existant : français, emojis en titres de section, blocs de code bash.
- Structure de `CLAUDE.md` : commandes groupées par catégorie avec commentaires inline.

**Test scenarios:**

- Test expectation: none — documentation, pas de logique comportementale. Vérification manuelle : un contributeur sur une machine sans Rust peut suivre les étapes et arriver à `cargo test` qui passe.

**Verification:**

- `CONTRIBUTING.md` contient une section sur l'installation de rustup avant la section "Signaler un bug".
- `make coverage` est documenté avec ses prérequis (llvm-tools + cargo-llvm-cov).
- Toutes les variables d'environnement de `CLAUDE.md` sont mentionnées.
- Le fichier ne duplique pas le contenu de `SETUP.md`.

## Sources & References

- Commandes canoniques : `CLAUDE.md`
- Cibles Makefile : `Makefile`
- Dépendances dev : `Cargo.toml`
- Prérequis coverage : `Makefile:help` (lignes ajoutées lors de la session précédente)
