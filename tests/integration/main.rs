//! Tests d'intégration des reconcilers.
//!
//! Exécution : `make test-integration-kind` (crée un cluster kind éphémère).
//! Tous les tests sont `#[ignore]` pour ne tourner que dans ce contexte.
//!
//! Organisation, identique pour chaque ressource :
//!
//! - `<ressource>/mod.rs`           : construction et nettoyage des objets de cette ressource
//! - `<ressource>/sync.rs`          : création, adoption, synchronisation
//! - `<ressource>/authorization.rs` : rôle du namespace, autorisations
//! - `<ressource>/errors.rs`        : prérequis manquants, erreurs de l'API Scaleway
//! - `<ressource>/deletion.rs`      : finalizer et suppression
//!
//! `support/` contient ce qui est partagé : connexion au cluster, namespaces de test,
//! fausses réponses de l'API Scaleway.
//!
//! Les namespaces, NamespaceRoles et Secrets IAM sont pré-créés par `k8s/test-fixtures.yaml`.
//! Chaque test crée ses propres ressources et les supprime en fin de test.

mod instance;
mod load_balancer;
mod scaleway_secret;
mod support;
