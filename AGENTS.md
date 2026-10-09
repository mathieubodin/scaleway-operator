# AGENTS.md

Guide pour les agents IA travaillant sur ce dépôt. Opérateur Kubernetes en Rust ([kube-rs](https://kube.rs/))
qui réconcilie des Custom Resources avec l'API Scaleway. Docs et commentaires en français.

## Commandes

`make` est le point d'entrée unique, `make help` liste toutes les cibles.

- Tests unitaires rapides : `make coverage-text`
- Lint et format (Rust, markdown, charts Helm) : `make check`, aussi lancé en pre-commit par prek
- Tests d'intégration sur cluster kind éphémère : `make test-integration-kind`

## Pièges non évidents

- **Deux sources pour les CRDs.** `make generate-crds` écrit dans `k8s/`, mais le chart déploie
  `charts/scaleway-operator-crds/templates/`, enrichi à la main (validations, exemples).
  Toute modification de `src/resources.rs` doit être reportée dans les deux.
- **RBAC à suivre.** Une nouvelle CRD ou sous-ressource impose de mettre à jour le ClusterRole
  de `charts/scaleway-operator/templates/`.
- **Constantes de domaine.** Le groupe d'API `scaleway.mathieubodin.io`, l'annotation
  `scaleway.mathieubodin.io/project-id` et les finalizers doivent rester identiques entre code, charts et docs.
- **Commits conventionnels obligatoires.** release-please calcule versions et changelogs à partir
  du type de commit. Ne jamais éditer un `CHANGELOG.md` à la main.

## Où trouver quoi

- Modules, boucle de réconciliation, gestion d'erreurs, ajout d'une ressource : [ARCHITECTURE.md](ARCHITECTURE.md)
- Environnement de dev, process GitHub, milestones, sub-issues : [CONTRIBUTING.md](CONTRIBUTING.md)
- Problèmes déjà résolus, classés par catégorie avec frontmatter YAML : `docs/solutions/`
- Installation et usage de l'opérateur : [README.md](README.md)

## GitHub Project v2

Board : <https://github.com/users/mathieubodin/projects/2>

- Lecture et mutations du board : préfixer `GH_TOKEN=$GH_PROJECT_TOKEN`, le token `gh` local n'a pas
  le scope `project`. Les autres commandes `gh` utilisent l'authentification locale.
- À la création d'une issue, renseigner Axis, Priority, Effort et Status. Valeurs et IDs :
  CONTRIBUTING.md, section Project Field IDs.
- Toute feature : une issue parent plus une sous-issue par unité d'implémentation (U1…Un).
- Toute PR référence son issue avec `Closes #N`, ce qui met à jour le Status du board.
- Milestone : l'agent propose la composition, le mainteneur valide avant création.
