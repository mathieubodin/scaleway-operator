# Guide de Contribution

Merci de vouloir contribuer au Scaleway Operator !

## Environnement de développement

### Prérequis

**Rust et Cargo** — installés via [rustup](https://rustup.rs) :

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

Vérifier : `rustc --version` (>= 1.80 requis — kube 3.x + schemars 1.x)

**Outils supplémentaires :**

```bash
# macOS
brew install kind helm kubectl
npm install -g markdownlint-cli2

# Linux
go install sigs.k8s.io/kind@latest
curl https://raw.githubusercontent.com/helm/helm/main/scripts/get-helm-3 | bash
# kubectl : voir https://kubernetes.io/docs/tasks/tools/install-kubectl-linux/
npm install -g markdownlint-cli2
```

Docker doit être disponible et démarré.

### Commandes de développement

Utiliser `make` comme point d'entrée unique (`make help` pour la liste complète) :

| Commande | Description |
| --- | --- |
| `make env-check` | Vérifie la conformité de l'environnement |
| `make coverage-text` | Tests unitaires (résultat terminal, rapide) |
| `make coverage` | Tests unitaires (rapport HTML) |
| `make coverage-json` | Tests unitaires (rapport JSON/CI) |
| `make test-integration-kind` | Tests d'intégration via cluster kind éphémère |
| `make build` | Construit le binaire |
| `make check` | Lint et format |
| `make generate-crds` | Régénère les manifests CRD depuis `src/resources.rs` |
| `make image-build` | Construit l'image Docker |
| `make image-push` | Construit et pousse l'image |
| `make deploy-crds` | Déploie les CRDs via Helm |
| `make deploy` | Déploie l'opérateur via Helm |
| `make deploy-status` | Affiche l'état du déploiement |
| `make clean` | Nettoie les artefacts |


### Tests d'intégration

Les tests vérifient `reconcile_instance` contre un vrai API server Kubernetes, avec l'API Scaleway mockée via mockito — aucune credential Scaleway réelle requise.

```bash
make test-integration-kind
```

Le script `scripts/test-integration-kind.sh` orchestre le cycle complet :

1. Crée le cluster kind et écrit le kubeconfig dans `.kube/kind-config`
2. Déploie les CRDs via `helm upgrade`
3. Applique `k8s/test-fixtures.yaml` via `kubectl`
4. Exporte `KUBECONFIG=.kube/kind-config` puis lance `cargo test` — c'est ce que `Client::try_default()` lit pour se connecter au cluster kind
5. Supprime le cluster (`trap EXIT`) et nettoie `.kube/kind-config`, même en cas d'échec

#### Architecture des fixtures

Les tests ne créent que des objets `Instance` — les namespaces, NamespaceRoles et Secrets sont pré-créés par `k8s/test-fixtures.yaml` (appliqué automatiquement par le script) :

| Namespace | Annotation | NamespaceRole | Secret IAM | Utilisé pour |
| --- | --- | --- | --- | --- |
| `scw-test-no-role` | UUID valide | aucune | — | NamespaceRole manquante |
| `scw-test-no-annotation` | aucune | Editor | — | Annotation manquante |
| `scw-test-invalid-uuid` | `"not-a-uuid"` | Editor | — | UUID invalide |
| `scw-test-no-secret` | UUID valide | Editor | — | Secret IAM absent |
| `scw-test-viewer` | UUID valide | Viewer | oui | Rôle lecture seule |
| `scw-test-editor` | UUID valide | Editor | oui | Happy path : finalizer, suppression, création, sync, adoption, erreurs (plusieurs tests) |

### Déploiement sur un cluster réel

#### Kubeconfig

```bash
KUBECONFIG=~/.kube/config make deploy-crds   # standard
HELM_EXTRA_FLAGS=--force make deploy-crds    # forcer une mise à jour
```

#### Credentials Scaleway

```bash
HELM_EXTRA_FLAGS="--set scaleway.token=<token> --set scaleway.organizationId=<uuid>" make deploy
```

#### RBAC requis (une fois par cluster)

`helm upgrade --install` stocke son état comme des Secrets dans `scaleway-system`, et les CRDs sont cluster-scoped. L'utilisateur Kubernetes doit avoir :

| Scope | Ressource | Verbes |
| --- | --- | --- |
| Cluster | `apiextensions.k8s.io/customresourcedefinitions` | get, list, create, update, patch, delete |
| Namespace `scaleway-system` | `secrets`, `configmaps` | get, list, watch, create, update, patch, delete |

Sur Scaleway Kapsule, le nom d'utilisateur est `scaleway:bearer:<uuid-du-token-iam>`.

## Signaler un bug

1. Vérifiez que le bug n'existe pas déjà dans les issues
2. Ouvrez une issue avec : description claire, étapes de reproduction, comportement attendu vs actuel, version de l'opérateur et de Kubernetes

## Roadmap

Le [Project v2](https://github.com/users/mathieubodin/projects/2) est la source de vérité pour la planification.
Chaque issue ouverte y est automatiquement ajoutée et classifiée selon 4 dimensions : axe stratégique, priorité, effort, et coût en tokens IA.

### Configurer le secret `PROJECT_TOKEN`

Les workflows de traçabilité (`auto-add-to-project`, `update-status-on-pr`, `parse-cost-comment`) requièrent un fine-grained PAT stocké comme secret `PROJECT_TOKEN`.

**1. Créer le PAT** — les fine-grained PATs ne supportent pas encore les projets personnels (user-owned). Utiliser un **classic PAT** : [github.com/settings/tokens/new](https://github.com/settings/tokens/new) :

| Champ | Valeur |
| --- | --- |
| Note | `scaleway-operator-project` |
| Expiration | No expiration (recommandé pour un token de CI solo) |
| Scopes | `project` (Full control of projects) |

**2. Ajouter le secret** — [Settings → Secrets → Actions → New](https://github.com/mathieubodin/scaleway-operator/settings/secrets/actions/new) :

| Champ | Valeur |
| --- | --- |
| Name | `PROJECT_TOKEN` |
| Secret | valeur du PAT généré |

Sans ce secret, les workflows s'arrêtent proprement avec un warning — aucun check ne bloque.

**Renouvellement** : si le PAT expire, les workflows se dégradent silencieusement (warning). Pour renouveler : générer un nouveau classic PAT avec le même scope `project`, puis mettre à jour le secret `PROJECT_TOKEN` dans [Settings → Secrets → Actions](https://github.com/mathieubodin/scaleway-operator/settings/secrets/actions).

## Proposer une fonctionnalité

1. Ouvrez une issue avec le label `enhancement`
2. Décrivez le cas d'usage et proposez une solution
3. Pour une fonctionnalité non triviale, décomposez-la en sous-issues natives (sub-issues) — une par unité d'implémentation cohérente, nommées `U1`, `U2`, etc.

### Décomposition en sub-issues

Le pattern standard pour une feature est :

```
#N  feat: titre parent (issue parente, pas de PR directe)
├── #N+1  feat(scope): U1 — première unité
├── #N+2  feat(scope): U2 — deuxième unité
└── ...
```

Les sub-issues s'ajoutent via l'UI GitHub (bouton "Create sub-issue" sur l'issue parente) ou via l'API :

```bash
gh api repos/mathieubodin/scaleway-operator/issues/<parent>/sub_issues \
  --method POST -f sub_issue_id=<child_number>
```

## Soumettre une PR

1. **Fork** le dépôt
2. **Créez une branche** (`git checkout -b feat/ma-fonctionnalite`)
3. **Committez** en conventional commits (`feat(scope): description`)
4. **Référencez l'issue** dans le corps de la PR avec `Closes #N` (met à jour le Status automatiquement)
5. **Poussez** votre branche et **ouvrez une PR** avec une description claire

**Checklist avant de soumettre :**

- [ ] `make check` passe sans warnings
- [ ] `make coverage-text` passe
- [ ] `make test-integration-kind` passe
- [ ] `make generate-crds` relancé si `src/resources.rs` modifié
- [ ] Documentation à jour

### Convention `/cost N`

Avant de merger une PR, commentez le coût en tokens IA consommés pour l'implémenter :

```
/cost 12500
```

Le chiffre doit être un entier sans séparateur de milliers, en début de ligne.
Ce commentaire met à jour automatiquement le champ **Tokens** du Project v2 pour chaque issue liée via `Closes/Fixes/Resolves`.
Si vous commentez plusieurs fois, le dernier `/cost` prévaut.

## Style de code

- `make check` pour formater et linter
- Pas de doc comments sur les fonctions internes (noms auto-documentés)
- Commits en [Conventional Commits](https://www.conventionalcommits.org/)
