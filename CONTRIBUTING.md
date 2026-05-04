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

| Commande | Description |
|---|---|
| `cargo build --release` | Compile le binaire |
| `cargo test` | Lance tous les tests |
| `cargo test <nom>` | Lance un test spécifique |
| `cargo fmt` | Formate le code |
| `cargo clippy -- -D warnings` | Lint strict |
| `make check` | fmt + clippy + cargo check en une commande |
| `make coverage` | Rapport de couverture HTML |
| `make coverage-open` | Rapport de couverture + ouverture navigateur |

### Prérequis optionnels

**Coverage de tests** (`make coverage`) :

```bash
rustup component add llvm-tools
cargo install cargo-llvm-cov
```

Le rapport HTML est généré dans `target/llvm-cov/html/index.html`.

**Déploiement Kubernetes** (`make deploy`, `make logs`) :

```bash
# kubectl >= 1.24
# https://kubernetes.io/docs/tasks/tools/
kubectl version --client
```

**Build Docker** (`make docker-build`) :

```bash
# Docker Engine ou Docker Desktop
docker --version
```

### Variables d'environnement

Requises uniquement pour exécuter l'opérateur (pas pour `cargo test`) :

| Variable | Obligatoire | Description |
|---|---|---|
| `SCALEWAY_TOKEN` | Oui | Token API Scaleway |
| `SCALEWAY_ORG_ID` | Oui | ID de l'organisation Scaleway |

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

- [ ] Code formaté (`cargo fmt`)
- [ ] Pas de warnings clippy (`cargo clippy`)
- [ ] Tests ajoutés/passent (`cargo test`)
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

- Utilisez `cargo fmt` pour formater
- Respectez les avertissements de `cargo clippy`
- Documentez avec des doc comments (`///`)

### Commits

```
Add support for LoadBalancer resources

- Implement LoadBalancer CRD
- Add Scaleway API client methods
- Add reconciliation logic

Fixes #123
```

## 🧪 Tests

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Run specific test
cargo test test_name
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
