---
title: "Commentaires inline cassent la continuation `\\` dans les commandes bash multi-lignes"
date: 2026-05-12
category: docs/solutions/conventions/
module: documentation
problem_type: convention
component: documentation
severity: medium
applies_when:
  - Écriture de blocs bash dans des fichiers markdown (README, CLAUDE.md, tutoriels)
  - Commandes helm, kubectl, ou toute commande à arguments multiples répartis sur plusieurs lignes
  - Ajout d'annotations ou d'étiquettes à des arguments spécifiques d'une commande
tags:
  - bash
  - markdown
  - documentation
  - readme
  - multiline-command
  - comment-syntax
---

# Commentaires inline cassent la continuation `\` dans les commandes bash multi-lignes

## Context

Lors de la réécriture de la section Installation du README, le plan prescrivait d'ajouter des commentaires distinctifs (`# crds`, `# operator`) aux arguments `--version` des commandes `helm upgrade` pour permettre de les distinguer si les deux charts divergeaient en version. L'implémentation initiale a utilisé la syntaxe backtick :

```bash
helm upgrade scaleway-operator-crds \
    oci://... \
    --version 0.1.6 `# crds` \   # ← CASSÉ : backtick exécute "# crds" comme sous-commande
    --namespace scaleway-system \
```

Le plan affirmait également : *"bash ignore les commentaires en fin de ligne de commandes multi-lignes avec `\`"* — cette affirmation est fausse.

## Guidance

Le `\` de continuation bash doit être le **caractère absolu de fin de ligne** (aucun espace, aucun commentaire, aucun backtick après). Deux placements valides :

**Option 1 — commentaire sur la dernière ligne de la commande (pas de continuation `\` sur cette ligne) :**

```bash
helm upgrade scaleway-operator-crds \
    oci://ghcr.io/mathieubodin/charts/scaleway-operator-crds \
    --version 0.1.6 \
    --namespace scaleway-system \
    --create-namespace \
    --install  # crds
```

**Option 2 — ligne de commentaire séparée avant la commande :**

```bash
# crds chart — version maintenue par release-please
helm upgrade scaleway-operator-crds \
    oci://ghcr.io/mathieubodin/charts/scaleway-operator-crds \
    --version 0.1.6 \
    --namespace scaleway-system \
    --create-namespace \
    --install
```

## Why This Matters

Deux erreurs distinctes sont possibles selon la syntaxe choisie :

| Syntaxe | Comportement réel |
|---------|------------------|
| `--version 0.1.6 # crds \` | `\` est dans le commentaire → pas de continuation → la ligne suivante est une nouvelle commande |
| `--version 0.1.6 \`# crds\` \` | Les backticks exécutent `# crds` comme sous-commande shell (erreur silencieuse ou inattendue) |

Ces erreurs n'apparaissent pas à la lecture mais cassent la commande si elle est copiée et exécutée telle quelle. Pour la documentation destinée à être copiée-collée, la validité bash est critique.

## When to Apply

- Chaque fois qu'on annote un argument dans un bloc bash multi-lignes dans un fichier markdown
- Avant de publier des commandes multi-lignes : vérifier avec `bash -n <(cat <<'EOF' ... EOF)` ou en collant dans un terminal avec `set -n`

## Examples

### Avant (cassé — deux formes)

```bash
# Forme 1 : backslash pas dernier caractère
helm upgrade scaleway-operator-crds \
    --version 0.1.6 # crds \
    --namespace scaleway-system \
    --install

# Forme 2 : backtick exécute une sous-commande
helm upgrade scaleway-operator-crds \
    --version 0.1.6 `# crds` \
    --namespace scaleway-system \
    --install
```

### Après (valide)

```bash
helm upgrade scaleway-operator-crds \
    oci://ghcr.io/mathieubodin/charts/scaleway-operator-crds \
    --version 0.1.6 \
    --namespace scaleway-system \
    --create-namespace \
    --install  # crds
```

## Related

- Plan d'implémentation : `docs/plans/2026-05-12-001-docs-readme-installation-improvements-plan.md` (section Risks — risk "Commentaires `# crds`/`# operator` brisent la syntaxe bash" : l'hypothèse de mitigation était incorrecte)
