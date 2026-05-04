# Guide de Contribution

Merci de vouloir contribuer au Scaleway Operator !

## 🛠️ Environnement de développement

### Prérequis obligatoires

**Rust et Cargo** — installés via [rustup](https://rustup.rs) :

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

Vérifier l'installation :

```bash
rustc --version   # >= 1.80 requis (kube 3.x + schemars 1.x)
cargo --version
```

### Commandes de développement

- Tester la conformite de l'environnement : `make env-check`
- Tester l'application                    : `make coverage`
- Construire le binaire                   : `make build`
- Lint et format                          : `make check`
- Nettoyer les artefacts                  : `make clean`
- Construire l'image                      : `make image-build`
- Distribuer l'image                      : `make image-push`
- Deployer les CRDS                       : `make deploy-crd`
- Deployer la stack operateur             : `make deploy`
- Verifier l'etat du deploiement          : `make deploy-status`

### Variables d'environnement

Requises uniquement pour exécuter l'opérateur :

| Variable          | Obligatoire | Description                   |
|-------------------|-------------|-------------------------------|
| `SCALEWAY_TOKEN`  | Oui         | Token API Scaleway            |
| `SCALEWAY_ORG_ID` | Oui         | ID de l'organisation Scaleway |

## 🐛 Signaler un bug

1. Vérifiez que le bug n'existe pas déjà dans les issues
2. Ouvrez une issue avec:
   - Description claire du problème
   - Étapes pour reproduire
   - Comportement attendu vs actuel
   - Version du operator et Kubernetes

## 💡 Proposer une fonctionnalité

1. Ouvrez une issue avec le label `enhancement`
2. Décrivez le cas d'usage
3. Proposez une solution ou une API

## 🚀 Soumettre une PR

1. **Fork** le dépôt
2. **Créez une branche** (`git checkout -b feature/amazing-feature`)
3. **Committez** vos changements (`git commit -m 'Add amazing feature'`)
4. **Poussez** votre branche (`git push origin feature/amazing-feature`)
5. **Ouvrez une PR** avec une description claire

## 📋 Checklist avant de soumettre une PR

- [ ] Code formaté, pas de warnings clippy (`make check`)
- [ ] Tests ajoutés/passent (`make coverage`)
- [ ] Documentation à jour
- [ ] Commit messages clairs et descriptifs
- [ ] Pas de dépendances non nécessaires ajoutées

## 🎨 Style de code

### Rust

```rust
// Comments on their own line
fn my_function(param: String) -> Result<String> {
    // Implementation
    Ok(param)
}
```

- Utilisez `make check` pour formater
- Respectez les avertissements de `make check`
- Documentez avec des doc comments (`///`)

### Commits

```text
Add support for LoadBalancer resources

- Implement LoadBalancer CRD
- Add Scaleway API client methods
- Add reconciliation logic

Fixes #123
```

## 📖 Documentation

- Mise à jour du README.md pour les nouvelles fonctionnalités
- Ajouter des exemples dans `k8s/examples.yaml`
- Ajouter les doc comments pour les fonctions publiques

## 🔄 Processus de review

1. Un mainteneur reviewera votre PR
2. Demandes de changements possibles
3. Une fois approuvée, elle sera mergée

## 📞 Questions ?

N'hésitez pas à:

- Ouvrir une issue avec la question
- Commenter sur une PR existante
- Demander de l'aide

Merci de votre contribution !
