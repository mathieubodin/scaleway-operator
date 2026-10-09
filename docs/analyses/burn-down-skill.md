---
title: "Analyse — skill Burn Down : état des issues et avancement"
date: 2026-10-08
status: draft
---

# Analyse — skill `burn-down`

Document de travail, à revoir ensemble avant toute planification.

## Objectif

Une skill Claude Code `burn-down` qui répond à deux questions, à la demande :

1. **Où en est-on ?** État des issues en cours : ce qui avance, ce qui bloque, ce qui dort.
2. **Avance-t-on ?** Tendance dans le temps : travail fait, travail restant, rythme, projection.

Partage des rôles :

- **La skill donne une vision locale et tabulaire.** Elle produit des tableaux dans le terminal,
  sans graphique. Elle apporte ce que le web ne montre pas : l'état « En PR », les écarts entre
  Status et PRs, la stagnation, les blocages.
- **Le Project GitHub porte les courbes.** Les graphiques d'avancement se consultent dans l'onglet
  Insights de <https://github.com/users/mathieubodin/projects/2>. La skill ne les reproduit pas.

Le périmètre initial est GitHub, seul tracker utilisé par le projet. La mention de Jira dans la demande
est traitée comme une question ouverte (voir [Questions ouvertes](#questions-ouvertes)).

## Ce que disent les données actuelles

Relevé du 2026-10-08 sur `mathieubodin/scaleway-operator`.

| Mesure | Valeur |
| --- | --- |
| Issues totales / ouvertes / fermées | 69 / 50 / 19 |
| Milestones | 1, `M1 — Couverture API Scaleway`, sans date d'échéance |
| Issues de M1 ouvertes / fermées | 20 / 3 |
| Issues parentes avec sous-issues | 7, toutes à 0 % de sous-issues fermées |
| PRs mergées / dont avec `Closes #N` | 46 / 5 |
| Dernière fermeture d'issue, dernier merge | 2026-06-12 |
| Éléments du Project GitHub « Scaleway Operator » | 69 |
| Status sur le board | Backlog 30, In Progress 18, Review 2, Done 19 |
| Champs Axis, Priority, Effort renseignés | 67 sur 69 |
| Champ Tokens renseigné | 0 sur 69 |

Constats qui conditionnent le design :

- **Le travail en cours est invisible au comptage des issues fermées.** La PR #113 (`feat/milestone-1`,
  32 commits, CI verte, mergeable, sans review) déclare `Closes` sur 18 des 20 issues ouvertes de M1.
  Son merge ferait passer M1 de 3 à 21 issues fermées d'un coup. Un burn-down classique afficherait
  une ligne plate pendant quatre mois puis une falaise. La skill doit distinguer un état « en PR ».
- **Le Status du board ne suffit pas, il faut le recouper.** Les 18 issues « In Progress » sont
  exactement celles couvertes par la PR #113, alors que le workflow `update-status-on-pr` aurait dû
  les passer en « Review ». À l'inverse, #100 et #119 sont en « Review » sans aucune PR.
  La skill doit confronter le Status déclaré à l'état réel des PRs et signaler les écarts.
- **Aucune activité depuis quatre mois.** Ni issue fermée ni PR mergée depuis le 2026-06-12. Détecter
  et signaler la stagnation est une fonction à part entière, pas un cas limite.
- **Le lien PR vers issue est rare.** 5 PRs sur 46 portent un `Closes #N`. Une partie de l'avancement
  passé n'est pas rattachable à une issue, et une issue a été créée et fermée le même jour après coup
  (#120, LoadBalancer). L'historique antérieur à juin sera peu fiable.
- **Les milestones n'ont pas d'échéance, par convention.** CONTRIBUTING.md les définit comme des
  « feature bundles » terminés quand leurs issues sont fermées. Il n'y a donc pas de ligne idéale
  au sens classique du burn-down. La projection doit venir du rythme observé, pas d'une date cible.
- **Le périmètre de M1 a bougé.** Sept issues y ont été ajoutées le 2026-06-12 (#114 à #120).
  Sans suivi du périmètre, un ajout ressemble à un recul de l'avancement.
- **Le milestone mélange des sujets.** Deux des trois issues fermées de M1 relèvent de l'outillage
  (tracking de tokens, STRATEGY.md), pas de l'axe « Couverture API ». La skill peut le signaler.

## Ce que la skill pourrait produire

Quatre vues, de la plus simple à la plus riche.

### Vue 1 : instantané de l'état

Pour un milestone, ou tout le backlog ouvert :

- répartition des issues par état dérivé : Backlog, En cours, En PR, En review, Fermée ;
- avancement des issues parentes par leurs sous-issues (`sub_issues_summary`) ;
- PRs ouvertes, avec issues couvertes, état de la CI, de la review et de mergeabilité ;
- issues sans activité depuis N jours.

L'état « En PR » est dérivé : issue ouverte et référencée par `Closes` dans une PR ouverte.
C'est ce qui rend visible le cas de la PR #113.

### Vue 2 : tableau de burn-up du milestone

Un tableau avec une ligne par semaine et des colonnes cumulées : périmètre total, terminé, en PR,
restant. Le format burn-up plutôt que burn-down sépare les ajouts de périmètre de l'avancement réel.
La colonne « en PR » est la valeur ajoutée par rapport au graphique du web, qui ne connaît que
l'état ouvert ou fermé des éléments.

La courbe correspondante se consulte dans Insights. Les deux doivent reposer sur le même périmètre
pour que leurs chiffres concordent : mêmes éléments, même milestone.

Reconstruction historique à partir de la timeline des issues :
les événements `milestoned` et `demilestoned` donnent le périmètre, `closed` et `reopened` le terminé.

### Vue 3 : rythme et projection

- débit hebdomadaire d'issues fermées, en moyenne glissante ;
- projection de fin du milestone au rythme observé, avec une fourchette ;
- alerte de stagnation si le débit est nul sur la fenêtre récente.

Avec un débit nul, la projection est indéfinie. La skill doit le dire tel quel, sans extrapoler.

### Vue 4 : pondération et coût

Le champ `Effort` du Project GitHub est renseigné sur 67 éléments sur 69 (S, M, L, XL). Il permet
de pondérer le burn-up par l'effort plutôt que par le nombre d'issues, avec une échelle à définir.
Le champ `Tokens` n'est renseigné nulle part : la convention `/cost N` n'a jamais été utilisée.
La vue coût n'a donc aucune donnée à ce jour.

## Sources de données et contraintes

| Source | Donne | Accès |
| --- | --- | --- |
| API REST issues et milestones | état, dates, milestone, labels | token `gh` local |
| `sub_issues_summary` | avancement des parents | token `gh` local |
| Timeline des issues | `milestoned`, `closed`, `reopened`, changements de Status | token `gh` local |
| PRs, `closingIssuesReferences` | état « En PR », CI, review | token `gh` local |
| Champs du Project GitHub (Status, Axis, Priority, Effort, Tokens) | valeurs courantes | token `gh` local avec scope `read:project` |
| GraphQL `ProjectV2ItemStatusChangedEvent` | historique du Status, ancienne et nouvelle valeur | token `gh` local avec scope `read:project` |

Contraintes relevées :

- **L'historique du Status passe par GraphQL.** La timeline REST date chaque changement sans en donner
  les valeurs. L'API GraphQL les fournit (`previousStatus`, `status`), vérifié sur l'issue #120.
  L'historique du Status est donc reconstructible sans instantanés périodiques.
- **Le scope `read:project` est requis sur le token `gh`.** Il a été ajouté le 2026-10-08 sur le poste
  du mainteneur. La lecture du board n'a plus besoin de `GH_PROJECT_TOKEN`. La skill doit vérifier
  ce scope au démarrage et dégrader proprement vers les vues sans board s'il manque.
- **Le volume reste faible.** Moins de 100 issues : un appel par issue pour la timeline est acceptable,
  sans cache, en quelques secondes.

## Forme technique envisagée

- **Emplacement** : `.claude/skills/burn-down/` dans le dépôt, pour qu'elle suive le projet.
  Alternative : skill globale dans `~/.claude/skills/` si elle doit servir à d'autres dépôts.
- **Contenu** : un `SKILL.md` qui décrit quand l'utiliser et comment lire le rapport, et un script
  `gh` plus `jq` qui collecte et agrège. Le script fait le calcul de manière déterministe,
  l'agent ne fait que l'interprétation et les recommandations.
- **Sortie** : des tableaux markdown dans le terminal, sans graphique. Option : un export JSON ou CSV
  des mêmes tableaux, pour réutilisation ou comparaison dans le temps.
- **Graphiques** : configurés une fois dans Insights du Project GitHub, hors de la skill.
  Candidats : burn-up filtré sur le milestone actif, répartition par Status, effort restant par Axis.
  Disponibilité des graphiques historiques sur un projet personnel privé : à vérifier dans
  l'interface, l'API ne les expose pas.
- **Paramètres** : milestone (par défaut, le milestone ouvert), fenêtre de rythme, seuil de stagnation.

Hors périmètre de cette skill : modifier les issues, le board ou les milestones. Elle lit et rend compte.

## Proposition de découpage

| Étape | Contenu | Prérequis |
| --- | --- | --- |
| MVP | Vue 1, instantané avec état « En PR », écarts de Status et stagnation | scope `read:project` |
| 2 | Vue 2, burn-up reconstruit depuis la timeline | aucun |
| 3 | Vue 3, rythme et projection | étape 2 |
| 4 | Vue 4, pondération par Effort | échelle de pondération à définir |
| 5 | Vue 4, coût en tokens | champ `Tokens` alimenté |

Le MVP apporte déjà la réponse la plus utile aujourd'hui : M1 est presque entièrement codé,
il attend une review et un merge.

## Actions de remise en ordre du dépôt GitHub

Ces actions sont indépendantes de la skill. Elles corrigent l'état actuel du dépôt et du Project GitHub,
pour que la skill mesure un projet sain plutôt que des incohérences. Relevé du 2026-10-08.

### A. Débloquer, en priorité

Constat structurant : la protection de `main` exige les checks `lint` et `unit-tests`, mais aucun
workflow présent sur `main` ne les produit. Ils viennent de `.github/workflows/pr.yml`, qui n'existe
que sur la branche `feat/milestone-1`. Toute PR qui n'en hérite pas reste bloquée.

| # | Action | Constat |
| --- | --- | --- |
| A1 | Reviewer et merger la PR #113, en suivant la checklist pré-merge du projet | 32 commits, +4195/−605, CI verte, mergeable, sans review depuis le 2026-06-13. Ferme 18 issues de M1 et apporte `pr.yml` |
| A2 | Relancer puis merger les PRs Dependabot une fois A1 fait | #121 à #124 en état `BLOCKED`, #93 en retard sur `main`. #93 est une montée de version majeure de `actions/add-to-project` |
| A3 | Vérifier que la release automatique passe la protection de `main` | La PR de release-please attend les mêmes checks. Le job `sync-readme-versions` pousse directement sur `main` avec `GITHUB_TOKEN`. Aucune release depuis le 2026-06-12, comportement non observé |
| A4 | Requalifier #100 (protection de branche) | La protection avec checks obligatoires est déjà active alors que l'issue est ouverte. L'activer avant le merge de `pr.yml` est la cause du blocage |

### B. Corriger le Project GitHub

| # | Action | Constat |
| --- | --- | --- |
| B1 | Repasser #100 et #119 du Status « Review » à leur état réel | Aucune PR ne les couvre |
| B2 | Ajouter le déclencheur `edited` au workflow `update-status-on-pr` | Il ne réagit qu'à `opened`, `reopened` et `closed`. Des `Closes` ajoutés après l'ouverture de la PR #113 expliquent probablement ses 18 issues restées « In Progress » |
| B3 | Retirer du board les issues de test #89 et #90 | Fermées, sans Axis, Priority ni Effort. Elles testaient l'identité agent supprimée |
| B4 | Configurer les graphiques dans Insights : burn-up du milestone actif, répartition par Status, effort par Axis | Les courbes d'avancement sont confiées au web, la skill reste tabulaire |

### C. Rationaliser ce qui ne sert pas

| # | Action | Constat |
| --- | --- | --- |
| C1 | Trancher sur le suivi de coût par `/cost N` : l'adopter vraiment ou supprimer le champ `Tokens`, le workflow `parse-cost-comment` et la section de CONTRIBUTING.md | Champ vide sur 69 éléments. L'issue #45 prévoyait une évaluation après deux à trois mois, on en est à quatre et demi. Les trailers git de tokens couvrent déjà le besoin par commit |
| C2 | Fermer #45 avec la décision prise en C1 | Ouverte depuis le 2026-05-24, jamais mise à jour |
| C3 | Supprimer le label `in progress` | Il double le Status du board |
| C4 | Supprimer les labels par défaut inutilisés si le projet reste solo | `good first issue`, `help wanted`, `question`, `invalid`, `wontfix`, `duplicate` |

### D. Rendre l'avancement traçable

| # | Action | Constat |
| --- | --- | --- |
| D1 | Ajouter un modèle de PR avec une ligne `Closes #` à compléter | 5 PRs mergées sur 46 portent un `Closes #N`. Sans ce lien, ni le board ni la skill ne voient l'avancement |
| D2 | Trier les 30 issues ouvertes hors milestone et préparer M2 à la clôture de M1 | Vault, RDB, Bucket, Redis et la jauge du circuit breaker (#24 à #26) n'ont pas bougé depuis mai |
| D3 | Réserver un milestone à son axe ou assumer un milestone mixte | M1 « Couverture API » contient des issues fermées d'outillage (#51, #106) |

### E. Nettoyer le dépôt

| # | Action | Constat |
| --- | --- | --- |
| E1 | Activer la suppression automatique des branches après merge | Réglage `delete_branch_on_merge` désactivé |
| E2 | Supprimer les branches distantes déjà mergées | `chore/migrate-token-tracking`, `docs/readme-installation-improvements`, `fix/ci-use-preinstalled-kind`, `fix/docker-toolchain-musl-targets`, `worktree-docs+strategy`, `worktree-update-claude-md` |
| E3 | Examiner puis supprimer les branches `worktree-*` non mergées | Quatre branches inactives depuis mai, restes de sessions d'agents |

### F. Remettre la documentation en cohérence

| # | Action | Constat |
| --- | --- | --- |
| F1 | Harmoniser le type de PAT décrit pour `GH_PROJECT_TOKEN` | CONTRIBUTING.md et trois workflows parlent d'un PAT « fine-grained », alors que la même section impose un PAT classique |
| F2 | Mettre à jour la règle d'accès au board dans AGENTS.md et CONTRIBUTING.md | La lecture passe par le token `gh` local avec `read:project`. Seules les mutations exigent encore `GH_PROJECT_TOKEN` ou le scope `project` |
| F3 | Écrire « Project GitHub » dans la prose et réserver « ProjectV2 » aux mentions de l'API | L'interface GitHub n'utilise pas le nom « Project v2 » |

### Lien avec la skill

Plusieurs de ces constats ont été trouvés à la main et peuvent devenir des contrôles automatiques
du rapport : PRs bloquées par des checks introuvables, écarts entre Status et PRs, issues sans
champs du board, PRs mergées sans `Closes`, branches mergées non supprimées. La skill garderait
ainsi le projet dans l'état obtenu après ces actions.

## Questions ouvertes

1. **Jira.** La demande mentionne « Jira et GitHub ». Le projet n'utilise pas Jira à ce jour.
   Faut-il prévoir une source Jira, pour ce projet ou pour d'autres ? Cela changerait l'architecture :
   un format commun d'issues en entrée, des adaptateurs par source.
2. **Unité d'avancement.** Nombre d'issues, effort pondéré, ou les deux ?
3. **Issues parentes.** Comptées comme une unité, ignorées au profit de leurs sous-issues,
   ou affichées à part ? Les compter en plus des sous-issues gonfle le périmètre.
4. **Périmètre hors milestone.** 30 issues ouvertes n'ont pas de milestone (Vault, RDB, Bucket, Redis,
   métriques). Une vue backlog global est-elle utile, ou seul le milestone actif compte ?
5. **Fréquence.** Usage à la demande uniquement, ou aussi un rapport périodique, par exemple une issue
   ou un commentaire hebdomadaire posté par un workflow GitHub Actions ?
6. **Portée.** Skill du dépôt ou skill globale réutilisable sur d'autres projets GitHub ?

## Décisions prises

- **2026-10-08 — Versionnement.** `docs/analyses/` est suivi par git et publié sur GitHub avec le projet.
  `docs/plans/` reste ignoré.
- **2026-10-08 — Partage des rôles.** La skill produit des tableaux en local, les courbes se consultent
  dans Insights du Project GitHub.
- **2026-10-08 — Accès à GitHub.** Aucun token personnel n'est stocké dans le dépôt. Les opérations qui demandent les droits
  du mainteneur, release comprise, passent par l'authentification `gh` du shell. Le board s'appuie sur les automatisations
  natives du Project GitHub, et le secret `GH_PROJECT_TOKEN` est voué à disparaître.
- **2026-10-08 — Suivi de coût.** La convention `/cost N` est conservée, exécutée depuis le shell.
