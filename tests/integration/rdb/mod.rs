//! Tests du reconciler RDB.
//!
//! Chaque test crée son propre CR, et au besoin son ScalewaySecret, pour pouvoir
//! tourner en parallèle. Les APIs RDB et Secret Manager sont simulées par mockito.

mod authorization;
mod deletion;
mod errors;
mod sync;

use crate::support::*;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{DeleteParams, Patch, PatchParams, PostParams};
use kube::Api;
use scaleway_operator::{
    context::Context,
    error::OperatorError,
    resources::{DeletionPolicy, Rdb, RdbSpec, RdbStatus, ScalewaySecretRef, ScalewaySecretStatus},
};
use std::sync::Arc;

pub const RDB_FINALIZER: &str = "scaleway.mathieubodin.io/rdb-finalizer";
/// Chemin de l'API RDB pour la région des tests.
pub const RDB_PATH: &str = "/rdb/v1/regions/fr-par/instances";

/// Recherche par tags : la requête porte `project_id` et `tags` en paramètres.
fn mock_find_by_tags(server: &mut mockito::Server, body: &str) -> mockito::Mock {
    server
        .mock("GET", RDB_PATH)
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
}

/// Lecture de la version `revision` du secret `secret_id`, dont la valeur est `S3cret!pass`.
fn mock_access_secret(
    server: &mut mockito::Server,
    secret_id: &str,
    revision: u32,
) -> mockito::Mock {
    mock_json(
        server,
        "GET",
        &format!(
            "/secret-manager/v1beta1/regions/fr-par/secrets/{secret_id}/versions/{revision}/access"
        ),
        r#"{"data": "UzNjcmV0IXBhc3M="}"#,
    )
}

/// Instance telle que la renvoie Scaleway, conforme au spec de `rdb_spec`.
fn instance_body(id: &str, state: &str) -> String {
    serde_json::json!({
        "id": id,
        "status": state,
        "engine": "PostgreSQL-16",
        "node_type": "DB-DEV-S",
        "is_ha_cluster": false,
        "volume": { "type": "bssd", "size": 20_000_000_000u64 },
        "endpoints": [{ "ip": "51.158.1.2", "port": 5432, "load_balancer": {} }],
    })
    .to_string()
}

fn rdb_spec(secret_name: &str) -> RdbSpec {
    RdbSpec {
        region: "fr-par".to_string(),
        engine: "PostgreSQL-16".to_string(),
        node_type: "DB-DEV-S".to_string(),
        admin_user: "admin".to_string(),
        admin_password_secret_ref: ScalewaySecretRef {
            name: secret_name.to_string(),
        },
        volume_size_gb: Some(20),
        volume_type: "bssd".to_string(),
        is_ha_cluster: false,
        disable_backup: false,
        tags: vec![],
        deletion_policy: DeletionPolicy::Delete,
    }
}

/// Status d'une instance déjà créée et connue de l'opérateur.
fn known_instance_status(scaleway_id: &str) -> RdbStatus {
    RdbStatus {
        scaleway_id: Some(scaleway_id.to_string()),
        state: "provisioning".to_string(),
        sync_state: "Syncing".to_string(),
        ..RdbStatus::default()
    }
}

impl TestFixture {
    /// Crée un CR RDB avec son finalizer, pour entrer directement dans la branche testée.
    pub async fn create_rdb(&self, name: &str, spec: RdbSpec) -> Rdb {
        let api: Api<Rdb> = Api::namespaced(self.client.clone(), self.ns);
        let obj = Rdb {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(self.ns.to_string()),
                finalizers: Some(vec![RDB_FINALIZER.to_string()]),
                ..Default::default()
            },
            spec,
            status: None,
        };
        api.create(&PostParams::default(), &obj)
            .await
            .unwrap_or_else(|e| panic!("create_rdb({}) failed: {}", name, e));
        self.get_rdb(name).await
    }

    /// Crée le ScalewaySecret référencé par un RDB, déjà `Synced` sur la révision donnée.
    /// Retourne son nom.
    pub async fn create_synced_password_secret(
        &self,
        rdb_name: &str,
        scaleway_id: &str,
        revision: u32,
    ) -> String {
        let name = format!("{rdb_name}-pwd");
        self.create_scaleway_secret(&name, "unused-source", "password")
            .await;
        self.set_scaleway_secret_status(
            &name,
            ScalewaySecretStatus {
                scaleway_id: Some(scaleway_id.to_string()),
                current_version: Some(revision),
                last_synced_resource_version: Some("1".to_string()),
                observed_generation: Some(1),
                sync_state: "Synced".to_string(),
                error_message: None,
            },
        )
        .await;
        name
    }

    pub async fn set_rdb_status(&self, name: &str, status: RdbStatus) {
        let api: Api<Rdb> = Api::namespaced(self.client.clone(), self.ns);
        let patch = serde_json::json!({ "status": status });
        api.patch_status(name, &PatchParams::default(), &Patch::Merge(patch))
            .await
            .unwrap_or_else(|e| panic!("set_rdb_status({}) failed: {}", name, e));
    }

    /// Modifie des champs du spec, et rend la réponse de l'API server telle quelle
    /// pour pouvoir vérifier un refus de validation.
    pub async fn patch_rdb_spec(
        &self,
        name: &str,
        spec: serde_json::Value,
    ) -> Result<Rdb, kube::Error> {
        let api: Api<Rdb> = Api::namespaced(self.client.clone(), self.ns);
        let patch = serde_json::json!({ "spec": spec });
        api.patch(name, &PatchParams::default(), &Patch::Merge(patch))
            .await
    }

    pub async fn get_rdb(&self, name: &str) -> Rdb {
        let api: Api<Rdb> = Api::namespaced(self.client.clone(), self.ns);
        api.get(name)
            .await
            .unwrap_or_else(|e| panic!("get_rdb({}) failed: {}", name, e))
    }

    /// Demande la suppression : le finalizer retient l'objet, qui reste lisible
    /// avec un deletionTimestamp.
    pub async fn request_rdb_deletion(&self, name: &str) {
        let api: Api<Rdb> = Api::namespaced(self.client.clone(), self.ns);
        api.delete(name, &DeleteParams::default())
            .await
            .unwrap_or_else(|e| panic!("request_rdb_deletion({}) failed: {}", name, e));
    }

    pub async fn rdb_exists(&self, name: &str) -> bool {
        let api: Api<Rdb> = Api::namespaced(self.client.clone(), self.ns);
        api.get_opt(name).await.expect("get_opt RDB").is_some()
    }

    /// Supprime un RDB (retire d'abord le finalizer pour ne pas bloquer la GC).
    pub async fn cleanup_rdb(&self, name: &str) {
        let api: Api<Rdb> = Api::namespaced(self.client.clone(), self.ns);
        let remove_finalizer = serde_json::json!({ "metadata": { "finalizers": null } });
        let _ = api
            .patch(
                name,
                &PatchParams::default(),
                &Patch::Merge(remove_finalizer),
            )
            .await;
        let _ = api.delete(name, &DeleteParams::default()).await;
    }

    /// Relit le CR puis lance une réconciliation, comme le ferait le controller.
    pub async fn reconcile_rdb(
        &self,
        name: &str,
        ctx: &Arc<Context>,
    ) -> Result<kube::runtime::controller::Action, OperatorError> {
        let fetched = self.get_rdb(name).await;
        scaleway_operator::reconcilers::reconcile_rdb(Arc::new(fetched), ctx.clone()).await
    }
}
