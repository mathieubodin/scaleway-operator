# Scaleway Kubernetes Operator

Un opérateur Kubernetes moderne écrit en **Rust** pour gérer les ressources Scaleway directement depuis Kubernetes.

## 🚀 Fonctionnalités

- ✅ **Gestion complète des instances Scaleway** (CRUD)
- ✅ **Support des projets Scaleway** (via NamespaceRole — voir Prérequis namespace)
- ✅ **Synchronisation automatique** de l'état
- ✅ **Finalizers pour suppression propre**
- ✅ **Validation de configuration**
- ✅ **Logging structuré et tracing**
- ✅ **Multi-zone et multi-région**

## 📋 Prérequis

- Kubernetes 1.19+
- Token API Scaleway avec les permission sets IAM suivants :
  - `InstancesFullAccess` (scope projet) — créer, lire, supprimer des instances
  - `ProjectReadOnly` (scope organisation) — vérifier l'accès au projet cible
- Rust 1.70+ (pour le build)

## 🛠️ Installation

### 1. Installer les CRDs

```bash
kubectl apply -f k8s/crd-instance.yaml
kubectl apply -f k8s/crd-project.yaml
kubectl apply -f k8s/crd-namespacerole.yaml
```

### 2. Configurer les credentials de l'opérateur

Créez un secret contenant le token Scaleway et l'organisation. Évitez d'écrire le token directement dans la commande (il apparaîtrait dans l'historique shell) :

```bash
# Lire le token depuis stdin sans l'exposer dans l'historique
kubectl create secret generic scaleway-credentials \
  --from-file=SCALEWAY_TOKEN=/dev/stdin \
  --from-literal=SCALEWAY_ORG_ID=<votre-org-id> \
  -n scaleway-system
# Saisir le token puis Ctrl+D
```

Le `deployment.yaml` doit injecter ces valeurs comme variables d'environnement dans le pod :

```yaml
env:
  - name: SCALEWAY_TOKEN
    valueFrom:
      secretKeyRef:
        name: scaleway-credentials
        key: SCALEWAY_TOKEN
  - name: SCALEWAY_ORG_ID
    valueFrom:
      secretKeyRef:
        name: scaleway-credentials
        key: SCALEWAY_ORG_ID
```

### 3. Configurer les prérequis namespace

Chaque namespace hébergeant des `Instance` doit avoir **deux prérequis** :

**a) Annoter le namespace avec le projet Scaleway cible :**

```bash
kubectl annotate namespace production \
  scaleway.io/project-id="12345678-1234-1234-1234-123456789012"
```

**b) Créer la ressource `NamespaceRole` (nom = nom du namespace) :**

```yaml
apiVersion: scaleway.io/v1
kind: NamespaceRole
metadata:
  name: production          # Doit correspondre exactement au nom du namespace
spec:
  namespace: production
  scaleway_role: Editor     # Editor / Admin / Viewer / SecurityResponsible / BillingViewer / BillingManager / OrganizationOwner
```

```bash
kubectl apply -f namespacerole-production.yaml
```

**c) Créer le Secret IAM pré-provisionné pour ce namespace :**

Un admin doit créer une IAM Application Scaleway avec `InstancesFullAccess` sur le projet du namespace, puis stocker sa clé secrète :

```bash
kubectl create secret generic scaleway-ns-creds-production \
  --from-literal=secret_key=<api-key-secret-du-namespace> \
  -n scaleway-system
```

> **Rôles et permissions d'écriture** : seuls `Editor`, `Admin` et `OrganizationOwner` permettent de créer des instances. Les rôles `Viewer`, `SecurityResponsible`, `BillingViewer` et `BillingManager` sont en lecture seule.

### 4. Déployer l'opérateur

```bash
# Build de l'image Docker
docker build -t scaleway-operator:latest .

# Pousser vers votre registre (requis pour les clusters multi-nœuds)
docker tag scaleway-operator:latest your-registry/scaleway-operator:latest
docker push your-registry/scaleway-operator:latest
```

Puis déployer dans Kubernetes :

```bash
kubectl apply -f k8s/deployment.yaml
```

### 5. Vérifier l'installation

```bash
# Vérifier que les CRDs sont installées
kubectl get crd | grep scaleway

# Vérifier que l'opérateur tourne
kubectl -n scaleway-system get deployment
kubectl -n scaleway-system logs -f deployment/scaleway-operator
```

## 📖 Utilisation

### Créer une instance

Le `project_id` et les credentials sont lus automatiquement depuis le namespace (annotation `scaleway.io/project-id` et Secret `scaleway-ns-creds-{namespace}`). Ne les mettez pas dans le spec de l'Instance.

```yaml
apiVersion: scaleway.io/v1
kind: Instance
metadata:
  name: my-web-server
  namespace: production     # Le namespace doit avoir l'annotation et le NamespaceRole
spec:
  name: web-server-prod
  zone: fr-par-1
  image: ubuntu-jammy
  instance_type: GP1-M
  tags:
    - prod
    - web
```

```bash
kubectl apply -f instance.yaml
```

### Vérifier le statut

```bash
# Lister les instances
kubectl get instances

# Détails complets
kubectl describe instance my-web-server

# Voir le statut en temps réel
kubectl get instances -w
```

### Supprimer une instance

```bash
kubectl delete instance my-web-server
```

L'opérateur supprimera automatiquement l'instance Scaleway correspondante.

## 🔧 Configuration avancée

### Configuration réseau

```yaml
spec:
  network:
    public_ip: true        # Assigner une IP publique
    enable_ipv6: true      # Activer IPv6
```

### Configuration de sécurité

```yaml
spec:
  security:
    enable_firewall: true  # Activer le firewall
```

### Taille personnalisée du volume

```yaml
spec:
  boot_volume_size: 100    # En GB (défaut: 20)
```

## 📊 Monitoring

### Logs de l'opérateur

```bash
kubectl -n scaleway-system logs -f deployment/scaleway-operator
```

### Events Kubernetes

```bash
kubectl describe instance my-web-server
# Voir les events en bas de la sortie
```

### Health check

L'opérateur expose un endpoint de santé sur le port `8080` qui retourne `ok` :

```bash
kubectl port-forward -n scaleway-system deployment/scaleway-operator 8080:8080
curl http://localhost:8080/
# → ok
```

## 🐛 Troubleshooting

### Instance n'est pas créée

```bash
# Vérifier les logs
kubectl -n scaleway-system logs deployment/scaleway-operator | grep ERROR

# Vérifier les events
kubectl describe instance my-instance
```

### Erreur: "No NamespaceRole found for namespace"

Le namespace n'a pas de ressource `NamespaceRole` associée. Créez-en une dont le `metadata.name` correspond exactement au nom du namespace (voir étape 3b de l'installation).

### Erreur: "Namespace must have annotation scaleway.io/project-id"

Annotez le namespace avec le projet Scaleway cible :

```bash
kubectl annotate namespace <votre-namespace> \
  scaleway.io/project-id="<uuid-du-projet>"
```

### Erreur: "Secret scaleway-ns-creds-X not found"

Créez le Secret IAM pré-provisionné pour ce namespace (voir étape 3c de l'installation).

### Erreur: "Project access denied"

- Vérifier que l'annotation `scaleway.io/project-id` du namespace contient le bon UUID
- Vérifier que le token API de l'opérateur a la permission `ProjectReadOnly`
- Vérifier que le projet existe dans Scaleway

### Erreur: "Role X is read-only and cannot create instances"

Le `scaleway_role` du `NamespaceRole` est un rôle en lecture seule. Utilisez `Editor`, `Admin` ou `OrganizationOwner` pour autoriser la création d'instances.

### Erreur: "Invalid zone" ou "Invalid instance type"

Zones valides :

- `fr-par-1`, `fr-par-2` (Paris)
- `nl-ams-1` (Amsterdam)
- `pl-waw-1` (Varsovie)
- `sg-sin-1` (Singapour)
- `it-mil-1` (Milan)

Types valides :

- `DEV1-S`, `DEV1-M`, `DEV1-L`, `DEV1-XL` (développement)
- `GP1-XS`, `GP1-S`, `GP1-M`, `GP1-L`, `GP1-XL` (généraliste)
- `CPU1-XS`, `CPU1-S`, `CPU1-M`, `CPU1-L` (CPU optimisé)
- `GPU-3090`, `GPU-4090` (GPU)

## 🏗️ Architecture

```text
┌─────────────────────────────────────────┐
│   Kubernetes Cluster                    │
│  ┌──────────────────────────────────┐   │
│  │  Scaleway Operator (Rust/kube-rs)│   │
│  │  - Watch Instance CRs            │   │
│  │  - Lit NamespaceRole + annotation│   │
│  │  - Reconcile avec Scaleway API   │   │
│  └──────────────────────────────────┘   │
└─────────────────────────────────────────┘
         │
         └─► Scaleway API
              - Create/Delete instances
              - Get status
              - Verify project access
```

## 📝 Structure du code

```text
scaleway-operator/
├── src/
│   ├── main.rs          # Point d'entrée
│   ├── error.rs         # Types d'erreur
│   ├── resources.rs     # Définition des CRDs (Instance, NamespaceRole)
│   ├── context.rs       # Contexte partagé + helpers annotations
│   ├── scaleway.rs      # Client Scaleway API
│   └── reconcilers.rs   # Logique de réconciliation
├── k8s/
│   ├── crd-instance.yaml       # CRD Instance
│   ├── crd-namespacerole.yaml  # CRD NamespaceRole (cluster-wide)
│   ├── deployment.yaml         # Deployment de l'opérateur
│   └── examples.yaml           # Exemples d'utilisation
├── Cargo.toml           # Dépendances Rust
├── Dockerfile           # Image Docker
└── README.md            # Ce fichier
```

## 🚀 Développement

### Build local

```bash
cargo build --release
# ou via Make :
make build
```

### Tests

```bash
cargo test
# ou avec coverage :
make coverage-json
```

### Format et lint

```bash
cargo fmt && cargo clippy
# ou via Make :
make check
```

## 📝 Roadmap

- [ ] Support des Load Balancers
- [ ] Support du Object Storage
- [ ] Support des bases de données managées
- [ ] Webhooks de validation avancée
- [ ] UI Web pour le management
- [ ] Support des snapshots
- [ ] Auto-scaling basé sur métriques

## 📄 Licence

MIT

## 🤝 Contribution

Les contributions sont bienvenues ! N'hésitez pas à ouvrir des issues ou PRs.

## 📞 Support

Pour toute question ou problème :

1. Vérifiez la documentation ci-dessus
2. Consultez les logs de l'opérateur
3. Ouvrez une issue sur le dépôt

## 🔗 Ressources

- [Documentation Scaleway API](https://developers.scaleway.com/)
- [Kube-rs - Rust Kubernetes client](https://kube.rs/)

## Deferred / Open Questions

### From 2026-05-03 review

- **[P3] Prérequis — Pas de recommendation de service account dédié** : L'opérateur peut utiliser un token personnel ou une IAM Application Scaleway.
    Une IAM Application dédiée est préférable (scope minimal, révocation sans impact sur l'utilisateur). À préciser dans les prérequis.
