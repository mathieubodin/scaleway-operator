//! Tests du reconciler Instance.

mod authorization;
mod deletion;
mod errors;
mod sync;

use crate::support::*;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{DeleteParams, Patch, PatchParams, PostParams};
use kube::Api;
use scaleway_operator::resources::{Instance, InstanceSpec, InstanceStatus};

pub const INSTANCE_FINALIZER: &str = "scaleway.mathieubodin.io/instance-finalizer";

/// Construit un objet Instance en mémoire (sans finalizer, sans status).
fn build_instance(ns: &str, name: &str) -> Instance {
    Instance {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            ..Default::default()
        },
        spec: InstanceSpec {
            name: name.to_string(),
            zone: "fr-par-1".to_string(),
            image: "ubuntu-jammy".to_string(),
            instance_type: "DEV1-S".to_string(),
            tags: vec![],
            boot_volume_size: 20,
            network: None,
            security: None,
        },
        status: None,
    }
}

/// Construit un objet Instance en mémoire avec le finalizer pré-présent.
fn build_instance_with_finalizer(ns: &str, name: &str) -> Instance {
    let mut instance = build_instance(ns, name);
    instance.metadata.finalizers = Some(vec![INSTANCE_FINALIZER.to_string()]);
    instance
}

impl TestFixture {
    /// Crée une Instance dans k8s et la retourne (re-fetched pour avoir resourceVersion).
    pub async fn create_instance(&self, name: &str) -> Instance {
        let api: Api<Instance> = Api::namespaced(self.client.clone(), self.ns);
        let obj = build_instance_with_finalizer(self.ns, name);
        api.create(&PostParams::default(), &obj)
            .await
            .unwrap_or_else(|e| panic!("create_instance({}) failed: {}", name, e));
        api.get(name)
            .await
            .unwrap_or_else(|e| panic!("get after create({}) failed: {}", name, e))
    }

    /// Crée une Instance dans k8s, patche son status, retourne l'objet à jour.
    pub async fn create_instance_with_status(
        &self,
        name: &str,
        status: InstanceStatus,
    ) -> Instance {
        let api: Api<Instance> = Api::namespaced(self.client.clone(), self.ns);
        let obj = build_instance_with_finalizer(self.ns, name);
        api.create(&PostParams::default(), &obj)
            .await
            .unwrap_or_else(|e| panic!("create_instance_with_status({}) failed: {}", name, e));

        let patch = serde_json::json!({ "status": status });
        api.patch_status(name, &PatchParams::default(), &Patch::Merge(patch))
            .await
            .unwrap_or_else(|e| panic!("patch_status({}) failed: {}", name, e));

        api.get(name)
            .await
            .unwrap_or_else(|e| panic!("get after patch_status({}) failed: {}", name, e))
    }

    /// Supprime une Instance (retire d'abord le finalizer pour ne pas bloquer la GC).
    pub async fn cleanup_instance(&self, name: &str) {
        let api: Api<Instance> = Api::namespaced(self.client.clone(), self.ns);
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
}
