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

## Project GitHub

Board : <https://github.com/users/mathieubodin/projects/2>

- Tout accès à GitHub passe par l'authentification `gh` locale, avec les scopes `repo`, `read:org`
  et `project`. Aucun token personnel n'est stocké dans le dépôt.
- À la création d'une issue, renseigner Axis, Priority, Effort et Status. Valeurs et IDs :
  CONTRIBUTING.md, section Project Field IDs.
- Toute feature : une issue parent plus une sous-issue par unité d'implémentation (U1…Un).
- Toute PR référence son issue avec `Closes #N`. Les automatisations natives du Project font suivre
  le Status : « Review » à la liaison, « Done » à la fermeture. Ne jamais passer une issue ouverte en « Done » à la main.
- Le champ `Tokens` des issues liées est renseigné par un hook après chaque `gh pr create` et `git push`.
  Si le hook n'est pas actif, lancer `make report-tokens`. Détail : CONTRIBUTING.md, section Convention `/cost N`.
- Après chaque merge `feat` ou `fix` sur `main`, proposer une release : `make release-prepare`, merge de la PR
  de release, puis `make release-publish`. Détail : CONTRIBUTING.md, section Publier une release.
- Milestone : l'agent propose la composition, le mainteneur valide avant création.
