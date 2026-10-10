//! Tests du reconciler ScalewaySecret.
//!
//! Chaque test crée son propre Secret source et son propre CR, pour pouvoir
//! tourner en parallèle. L'API Secret Manager est simulée par mockito.

mod authorization;
mod deletion;
mod errors;
mod sync;

use crate::support::*;
use k8s_openapi::api::core::v1::Secret;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::ByteString;
use kube::api::{DeleteParams, Patch, PatchParams, PostParams};
use kube::Api;
use scaleway_operator::{
    context::Context,
    error::OperatorError,
    resources::{
        KubernetesSecretRef, ScalewaySecret, ScalewaySecretSpec, ScalewaySecretStatus, SecretSource,
    },
};
use std::collections::BTreeMap;
use std::sync::Arc;

pub const SECRET_FINALIZER: &str = "scaleway.mathieubodin.io/secret-finalizer";
pub const OPT_IN_LABEL: &str = "scaleway.mathieubodin.io/allow-operator-read";
pub const ALLOWED_CR_ANNOTATION: &str = "scaleway.mathieubodin.io/allowed-cr";
/// Chemin de l'API Secret Manager pour la région des tests.
pub const SECRETS_PATH: &str = "/secret-manager/v1beta1/regions/fr-par/secrets";

/// Recherche par tags : la requête porte `project_id` et `tags` en paramètres.
fn mock_find_by_tags(server: &mut mockito::Server, body: &str) -> mockito::Mock {
    server
        .mock("GET", SECRETS_PATH)
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
}

fn synced_status(scaleway_id: &str, revision: u32, resource_version: &str) -> ScalewaySecretStatus {
    ScalewaySecretStatus {
        scaleway_id: Some(scaleway_id.to_string()),
        current_version: Some(revision),
        last_synced_resource_version: Some(resource_version.to_string()),
        sync_state: "Synced".to_string(),
        error_message: None,
        observed_generation: None,
    }
}

impl TestFixture {
    /// Crée un ScalewaySecret référençant un Secret K8s du namespace fixture (issue #118).
    ///
    /// `name`            : nom du CR ScalewaySecret (et du Secret Scaleway côté API).
    /// `k8s_secret_name` : nom du Secret K8s source dans le même namespace.
    /// `key`             : clé du Secret K8s à synchroniser.
    pub async fn create_scaleway_secret(
        &self,
        name: &str,
        k8s_secret_name: &str,
        key: &str,
    ) -> ScalewaySecret {
        self.create_scaleway_secret_in_region(name, k8s_secret_name, key, "fr-par")
            .await
    }

    pub async fn create_scaleway_secret_in_region(
        &self,
        name: &str,
        k8s_secret_name: &str,
        key: &str,
        region: &str,
    ) -> ScalewaySecret {
        let api: Api<ScalewaySecret> = Api::namespaced(self.client.clone(), self.ns);
        let obj = ScalewaySecret {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(self.ns.to_string()),
                finalizers: Some(vec![SECRET_FINALIZER.to_string()]),
                ..Default::default()
            },
            spec: ScalewaySecretSpec {
                name: name.to_string(),
                region: region.to_string(),
                source: SecretSource {
                    kubernetes_secret: Some(KubernetesSecretRef {
                        name: k8s_secret_name.to_string(),
                        key: key.to_string(),
                    }),
                },
                description: None,
                tags: vec![],
            },
            status: None,
        };
        api.create(&PostParams::default(), &obj)
            .await
            .unwrap_or_else(|e| panic!("create_scaleway_secret({}) failed: {}", name, e));
        api.get(name)
            .await
            .unwrap_or_else(|e| panic!("get after create({}) failed: {}", name, e))
    }

    /// Supprime un ScalewaySecret (retire d'abord le finalizer pour ne pas bloquer la GC).
    pub async fn cleanup_scaleway_secret(&self, name: &str) {
        let api: Api<ScalewaySecret> = Api::namespaced(self.client.clone(), self.ns);
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

    /// Crée le Secret K8s source d'un ScalewaySecret, avec la clé `key`.
    /// `opt_in` pose le label et l'annotation qui autorisent le CR `cr_name` à le lire.
    /// Retourne son nom et son resourceVersion.
    pub async fn create_source_secret(
        &self,
        cr_name: &str,
        key: &str,
        opt_in: bool,
    ) -> (String, String) {
        self.create_source_secret_with_keys(cr_name, &[key], opt_in)
            .await
    }

    /// Variante à plusieurs clés. La valeur de chaque clé est `value-of-<clé>`,
    /// pour pouvoir vérifier laquelle est envoyée à Scaleway.
    pub async fn create_source_secret_with_keys(
        &self,
        cr_name: &str,
        keys: &[&str],
        opt_in: bool,
    ) -> (String, String) {
        let api: Api<Secret> = Api::namespaced(self.client.clone(), self.ns);
        let name = format!("{}-src", cr_name);
        let (labels, annotations) = if opt_in {
            (
                Some(BTreeMap::from([(
                    OPT_IN_LABEL.to_string(),
                    "true".to_string(),
                )])),
                Some(BTreeMap::from([(
                    ALLOWED_CR_ANNOTATION.to_string(),
                    format!("{}/{}", self.ns, cr_name),
                )])),
            )
        } else {
            (None, None)
        };
        let obj = Secret {
            metadata: ObjectMeta {
                name: Some(name.clone()),
                namespace: Some(self.ns.to_string()),
                labels,
                annotations,
                ..Default::default()
            },
            data: Some(
                keys.iter()
                    .map(|key| {
                        (
                            key.to_string(),
                            ByteString(format!("value-of-{key}").into_bytes()),
                        )
                    })
                    .collect(),
            ),
            ..Default::default()
        };
        let created = api
            .create(&PostParams::default(), &obj)
            .await
            .unwrap_or_else(|e| panic!("create_source_secret({}) failed: {}", name, e));
        let rv = created
            .metadata
            .resource_version
            .expect("source secret has a resourceVersion");
        (name, rv)
    }

    pub async fn cleanup_source_secret(&self, name: &str) {
        let api: Api<Secret> = Api::namespaced(self.client.clone(), self.ns);
        let _ = api.delete(name, &DeleteParams::default()).await;
    }

    /// Écrit le status d'un ScalewaySecret, pour partir d'un état déjà synchronisé.
    pub async fn set_scaleway_secret_status(&self, name: &str, status: ScalewaySecretStatus) {
        let api: Api<ScalewaySecret> = Api::namespaced(self.client.clone(), self.ns);
        let patch = serde_json::json!({ "status": status });
        api.patch_status(name, &PatchParams::default(), &Patch::Merge(patch))
            .await
            .unwrap_or_else(|e| panic!("set_scaleway_secret_status({}) failed: {}", name, e));
    }

    pub async fn get_scaleway_secret(&self, name: &str) -> ScalewaySecret {
        let api: Api<ScalewaySecret> = Api::namespaced(self.client.clone(), self.ns);
        api.get(name)
            .await
            .unwrap_or_else(|e| panic!("get_scaleway_secret({}) failed: {}", name, e))
    }

    /// Relit le CR puis lance une réconciliation, comme le ferait le controller.
    pub async fn reconcile_scaleway_secret(
        &self,
        name: &str,
        ctx: &Arc<Context>,
    ) -> Result<kube::runtime::controller::Action, OperatorError> {
        let fetched = self.get_scaleway_secret(name).await;
        scaleway_operator::reconcilers::reconcile_scaleway_secret(Arc::new(fetched), ctx.clone())
            .await
    }
}
