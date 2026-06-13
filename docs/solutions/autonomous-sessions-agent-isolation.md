---
module: process
tags: [autonomous-sessions, context-window, agent-isolation, workflow, compaction]
problem_type: architecture
---

# Sessions autonomes — isolation par agent et gestion du contexte

## Problème

Une session autonome longue (ex : milestone entier) déclenche une **compaction de contexte** (~200K tokens). La compaction préserve les artefacts git et MemPalace, mais détruit l'état mental de l'agent : "où j'en suis", décisions prises, plan en cours.

Sans mécanisme de reprise, l'agent peut redémarrer aveuglément ou perdre le fil du milestone. Session de référence : milestone-1, 12 commits, ~1680 lignes, 230–350K tokens estimés. `reconcilers.rs` (2673 lignes) lu 3–4× en entier — principale source de dépassement.

## Dynamiques en jeu

### Consommation de contexte

| Source | Coût estimé | Contrôlable |
| --- | --- | --- |
| Lectures répétées de fichiers larges (>500 lignes) | 10–27K/lecture | Oui — `offset`/`limit` |
| Résultats d'agents (reviews, analyse) | 5–15K/agent | Oui — schema structuré |
| Sorties compile accumulées | 3–8K/cycle | Oui — seulement les erreurs |
| Historique de conversation | accumulation linéaire | Non |
| Overhead système | ~45K constant | Non |

Budget effectif par session : ~160K tokens (80% de 200K).

### Compaction

La compaction préserve : commits git, MemPalace, artefacts fichiers.
La compaction détruit : l'état in-context de l'agent.

Le hook `precompact` s'exécute automatiquement mais écrit un snapshot générique. Un état structuré de session (unité en cours, unités terminées, prochaine action) rendrait la reprise automatique.

### Reprise après compaction

Condition nécessaire : toute reprise doit commencer par `mempalace_diary_read`. Ne jamais supposer l'état depuis le résumé de compaction seul.

## Solution : isolation par agent (approche cible)

L'outil `Workflow` crée des sous-agents avec une **fenêtre de contexte vierge de 200K**. Le coordinateur ne reçoit que le résultat final — pas l'historique de travail.

```text
Coordinateur (contexte minimal, ~20–40K)
  └── Agent U1 (200K frais) : lit code → implémente → compile → teste → commit
  └── Agent U2 (200K frais) : briefé sur l'état post-U1
  └── Agent U3 (200K frais) : …
```

La compaction du coordinateur ne détruit plus rien d'important : l'état réel est dans git + MemPalace, et les agents ont terminé leur travail.

### Architecture Workflow

```javascript
phase("Plan")
const plan = await agent(
  "Lis le plan MemPalace + git log, retourne les unités à implémenter",
  { schema: PLAN_SCHEMA }
)

phase("Implement")
let previousResult = null
for (const unit of plan.units) {
  previousResult = await agent(
    buildBriefing(unit, previousResult, repoContext),
    { label: `impl:${unit.id}` }
  )
}
```

### Le briefing — défi central non résolu

Un sous-agent commence sans mémoire du projet. Un briefing pauvre coûte ses 200K en exploration exploratoire. Le template doit contenir :

- Architecture (CLAUDE.md condensé)
- État git actuel (branche, derniers commits, fichiers modifiés)
- Description précise de l'issue (titre, critères d'acceptation)
- Fichiers à lire et modifier (paths exacts, sections pertinentes)
- Patterns à suivre (extraits docs/solutions/ applicables)
- Commandes build/test
- Résultat de l'unité précédente (types exposés, signatures, décisions)

**Question ouverte** : peut-on générer automatiquement ce briefing depuis l'issue GitHub + l'état du dépôt ? Si oui, un milestone entier devient un seul appel `Workflow` sans intervention humaine.

Ce template fera l'objet d'une conception dédiée lors de la phase Compound.

## Règles transitoires (avant template Workflow)

En attendant l'approche agent-isolation, ces règles réduisent le risque de compaction dans une session mono-agent :

1. **Plan externalisé** : avant la première ligne de code, écrire le plan complet (unités + statuts) dans le diary MemPalace.
2. **Mise à jour après chaque commit** : `mempalace_diary_write` avec hash de commit et statut `done`.
3. **Reprise consciente** : si compaction détectée (résumé en tête de contexte), lire le diary avant toute action.
4. **Lectures ciblées** : jamais de lecture complète d'un fichier >500 lignes si seule une section est nécessaire ; jamais relire un fichier déjà lu dans la même unité.
5. **PushNotification** : à chaque fin d'unité, compaction détectée, ou blocage.

## Lien avec la phase Compound

Les sujets à traiter lors de la conception de l'orchestration Compound :

- Template de briefing auto-généré depuis une issue GitHub
- Format de `PLAN_SCHEMA` et `previousResult` pour le passage d'état inter-agents
- Gestion des dépendances entre unités d'un même milestone (séquentielles vs parallélisables)
- Enrichissement du hook `precompact` pour écrire un état de session structuré
- Stratégie de rollback si un agent échoue en milieu d'implémentation
