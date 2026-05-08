use crate::context::Context;
use crate::context::{extract_project_id_from_namespace, get_scaleway_role_for_namespace};
use crate::error::{OperatorError, Result};
use crate::resources::{Instance, InstanceStatus};
use crate::scaleway::ScalewayClient;
use chrono::Utc;
use k8s_openapi::api::core::v1::Secret;
use kube::api::Patch;
use kube::runtime::controller::Action;
use kube::{api::PatchParams, Api, ResourceExt};
use std::sync::Arc;
use std::time::Duration;

const INSTANCE_FINALIZER: &str = "scaleway.mathieubodin.io/instance-finalizer";
const NAMESPACE_CREDS_NS: &str = "scaleway-system";

/// Retourne true si le rôle autorise les opérations d'écriture sur les instances.
fn role_allows_write(role: &str) -> bool {
    matches!(role, "Editor" | "Admin" | "OrganizationOwner")
}

/// Lit les credentials IAM pré-provisionnés pour ce namespace depuis un Secret Kubernetes.
///
/// Convention : Secret `scaleway-ns-creds-{namespace}` dans `scaleway-system`,
/// champ `secret_key` contenant la clé secrète de l'API Key Scaleway IAM scopée.
/// Ce Secret doit être créé par un admin avant toute réconciliation d'instances.
async fn get_namespace_client(ctx: &Arc<Context>, namespace: &str) -> Result<ScalewayClient> {
    let secret_name = format!("scaleway-ns-creds-{}", namespace);
    let secrets_api: Api<Secret> = Api::namespaced(ctx.client.clone(), NAMESPACE_CREDS_NS);

    let secret = secrets_api.get(&secret_name).await.map_err(|_| {
        OperatorError::ConfigError(format!(
            "Secret '{secret_name}' not found in namespace '{NAMESPACE_CREDS_NS}'. \
             An admin must pre-provision IAM credentials for this namespace.",
        ))
    })?;

    let secret_key = secret
        .data
        .as_ref()
        .and_then(|d| d.get("secret_key"))
        .ok_or_else(|| {
            OperatorError::ConfigError(
                format!("Secret '{secret_name}' has no 'secret_key' field.",),
            )
        })
        .and_then(|bytes| {
            String::from_utf8(bytes.0.clone()).map_err(|_| {
                OperatorError::ConfigError(format!(
                    "Secret '{secret_name}': 'secret_key' is not valid UTF-8.",
                ))
            })
        })?;

    Ok(ScalewayClient::new_with_base_url(
        secret_key,
        ctx.scaleway_base_url.clone(),
    ))
}

/// Récupérer le project_id depuis l'annotation du namespace
async fn get_project_id_from_namespace(instance: &Instance, ctx: &Arc<Context>) -> Result<String> {
    let namespace = instance.namespace().unwrap_or_default();
    let api: Api<k8s_openapi::api::core::v1::Namespace> = Api::all(ctx.client.clone());

    let ns = api.get(&namespace).await.map_err(|e| {
        tracing::error!(namespace = %namespace, error = %e, "Failed to get namespace");
        OperatorError::ConfigError(format!("Cannot access namespace {}: {}", namespace, e))
    })?;

    let annotations = ns.annotations();

    extract_project_id_from_namespace(annotations).ok_or_else(|| {
        tracing::error!(
            namespace = %namespace,
            "Namespace missing required annotation: scaleway.mathieubodin.io/project-id"
        );
        OperatorError::ConfigError(format!(
            "Namespace '{}' must have annotation 'scaleway.mathieubodin.io/project-id'",
            namespace
        ))
    })
}

pub async fn reconcile_instance(
    instance: Arc<Instance>,
    ctx: Arc<Context>,
) -> std::result::Result<Action, OperatorError> {
    let namespace = instance.namespace().unwrap_or_default();
    let api: Api<Instance> = Api::namespaced(ctx.client.clone(), &namespace);

    tracing::info!(
        name = %instance.name_any(),
        namespace = %namespace,
        "Reconciling instance"
    );

    // 1. Suppression en priorité — avant tout lookup de ressources potentiellement absentes
    if instance.metadata.deletion_timestamp.is_some() {
        return handle_deletion(&instance, &api, &ctx).await;
    }

    // 2. Obtenir le rôle Scaleway depuis la ressource NamespaceRole
    let scaleway_role = match get_scaleway_role_for_namespace(&ctx.client, &namespace).await {
        Ok(role) => role,
        Err(e) => {
            tracing::error!(
                name = %instance.name_any(),
                namespace = %namespace,
                error = %e,
                "Cannot proceed without NamespaceRole"
            );
            let mut status = instance.status.clone().unwrap_or_default();
            status.error_message = Some(e.for_status());
            status.sync_state = "Error".to_string();
            let _ = update_status(&instance, &api, status).await;
            return Err(e);
        }
    };

    // 3. Obtenir le project_id depuis l'annotation du namespace
    let project_id = match get_project_id_from_namespace(&instance, &ctx).await {
        Ok(pid) => {
            // Valider le format UUID pour prévenir toute injection dans les URLs Scaleway
            if uuid::Uuid::parse_str(&pid).is_err() {
                let e = OperatorError::ConfigError(format!(
                    "Annotation 'scaleway.mathieubodin.io/project-id' must be a valid UUID, got: '{}'",
                    pid
                ));
                let mut status = instance.status.clone().unwrap_or_default();
                status.error_message = Some(e.for_status());
                status.sync_state = "Error".to_string();
                let _ = update_status(&instance, &api, status).await;
                return Err(e);
            }
            pid
        }
        Err(e) => {
            tracing::error!(
                name = %instance.name_any(),
                error = %e,
                "Cannot proceed without project_id from namespace annotation"
            );
            let mut status = instance.status.clone().unwrap_or_default();
            status.error_message = Some(e.for_status());
            status.sync_state = "Error".to_string();
            let _ = update_status(&instance, &api, status).await;
            return Err(e);
        }
    };

    tracing::info!(
        name = %instance.name_any(),
        namespace = %namespace,
        role = %scaleway_role,
        "Using Scaleway role for namespace"
    );

    // 4. Ajouter le finalizer si absent
    if !instance
        .metadata
        .finalizers
        .as_ref()
        .unwrap_or(&vec![])
        .contains(&INSTANCE_FINALIZER.to_string())
    {
        add_finalizer(&instance, &api).await?;
        return Ok(Action::requeue(Duration::from_secs(5)));
    }

    // 5. Valider la spec
    validate_spec(&instance.spec, &ctx.scaleway_client).await?;

    // 6. Lire les credentials IAM pré-provisionnés pour ce namespace
    let ns_client = match get_namespace_client(&ctx, &namespace).await {
        Ok(client) => client,
        Err(e) => {
            tracing::error!(name = %instance.name_any(), namespace = %namespace, error = %e, "Missing pre-provisioned IAM credentials");
            let mut status = instance.status.clone().unwrap_or_default();
            status.error_message = Some(e.for_status());
            status.sync_state = "Error".to_string();
            let _ = update_status(&instance, &api, status).await;
            return Err(e);
        }
    };

    // 7. Récupérer le statut actuel
    let mut status = instance.status.clone().unwrap_or_default();

    // 8. Créer l'instance si elle n'existe pas
    if status.scaleway_id.is_none() {
        // Bloquer les opérations d'écriture pour les rôles en lecture seule
        if !role_allows_write(&scaleway_role) {
            let e = OperatorError::ConfigError(format!(
                "Role '{}' is read-only and cannot create instances. Use 'Editor' or 'Admin'.",
                scaleway_role
            ));
            let mut status = instance.status.clone().unwrap_or_default();
            status.error_message = Some(e.for_status());
            status.sync_state = "Error".to_string();
            let _ = update_status(&instance, &api, status).await;
            return Err(e);
        }

        // Vérifier l'accès projet uniquement à la création (pas à chaque sync de 30s)
        ctx.scaleway_client
            .verify_project_access(&project_id)
            .await?;

        // Cherche d'abord une instance existante par nom : récupère une instance
        // orpheline si le status n'a pas pu être écrit lors d'une réconciliation précédente.
        let instance_id = match ns_client
            .find_instance_by_name(&instance.spec.zone, &instance.spec.name, &project_id)
            .await?
        {
            Some(existing_id) => {
                tracing::warn!(
                    name = %instance.name_any(),
                    scaleway_id = %existing_id,
                    "Adopted existing Scaleway instance (status write may have failed previously)"
                );
                existing_id
            }
            None => {
                tracing::info!(name = %instance.name_any(), project_id = %project_id, "Creating new Scaleway instance");
                match ns_client.create_instance(&instance.spec, &project_id).await {
                    Ok(id) => id,
                    Err(e) => {
                        tracing::error!(name = %instance.name_any(), error = %e, "Failed to create instance");
                        status.error_message = Some(e.for_status());
                        status.sync_state = "Error".to_string();
                        update_status(&instance, &api, status).await?;
                        return Err(e);
                    }
                }
            }
        };

        status.scaleway_id = Some(instance_id);
        status.state = "creating".to_string();
        status.created_at = Some(Utc::now());
        status.sync_state = "Syncing".to_string();
        status.error_message = None;
        status.project_id = Some(project_id.clone());

        update_status(&instance, &api, status.clone()).await?;
        return Ok(Action::requeue(Duration::from_secs(10)));
    }

    // 9. Synchroniser l'état depuis Scaleway
    if let Some(instance_id) = &status.scaleway_id.clone() {
        match ns_client
            .get_instance(&instance.spec.zone, instance_id, &project_id)
            .await
        {
            Ok(info) => {
                status.state = info.state.clone();
                status.public_ip = info.public_ip;
                status.project_id = Some(project_id.clone());
                status.sync_state = "Synced".to_string();
                status.error_message = None;
                update_status(&instance, &api, status).await?;
            }
            Err(OperatorError::InstanceNotFound(_)) => {
                tracing::warn!(name = %instance.name_any(), "Instance not found in Scaleway — will recreate");
                status.scaleway_id = None;
                status.state = "unknown".to_string();
                status.public_ip = None;
                status.error_message = None;
                status.sync_state = "Syncing".to_string();
                update_status(&instance, &api, status).await?;
                return Ok(Action::requeue(Duration::from_secs(5)));
            }
            Err(e) => {
                tracing::warn!(name = %instance.name_any(), error = %e, "Failed to sync instance status");
                status.error_message = Some(e.for_status());
                status.sync_state = "Error".to_string();
                update_status(&instance, &api, status).await?;
                return Err(e);
            }
        }
    }

    Ok(Action::requeue(Duration::from_secs(30)))
}

async fn handle_deletion(
    instance: &Instance,
    api: &Api<Instance>,
    ctx: &Arc<Context>,
) -> std::result::Result<Action, OperatorError> {
    tracing::info!(
        name = %instance.name_any(),
        "Deleting instance"
    );

    if let Some(status) = &instance.status {
        if let Some(instance_id) = &status.scaleway_id {
            let namespace = instance.namespace().unwrap_or_default();
            let ns_client = get_namespace_client(ctx, &namespace).await?;
            match ns_client
                .delete_instance(&instance.spec.zone, instance_id)
                .await
            {
                Ok(_) => {
                    tracing::info!(
                        name = %instance.name_any(),
                        instance_id = %instance_id,
                        "Successfully deleted Scaleway instance"
                    );
                }
                Err(e) => {
                    tracing::error!(
                        name = %instance.name_any(),
                        error = %e,
                        "Failed to delete Scaleway instance"
                    );
                    return Err(e);
                }
            }
        }
    }

    // Supprimer le finalizer
    let finalizers = instance.metadata.finalizers.clone().unwrap_or_default();
    let new_finalizers: Vec<String> = finalizers
        .into_iter()
        .filter(|f| f != INSTANCE_FINALIZER)
        .collect();

    let patch = serde_json::json!({
        "metadata": {
            "finalizers": new_finalizers
        }
    });

    api.patch(
        &instance.name_any(),
        &PatchParams::default(),
        &Patch::Merge(patch),
    )
    .await?;

    Ok(Action::await_change())
}

async fn add_finalizer(instance: &Instance, api: &Api<Instance>) -> Result<()> {
    let mut finalizers = instance.metadata.finalizers.clone().unwrap_or_default();
    finalizers.push(INSTANCE_FINALIZER.to_string());

    let patch = serde_json::json!({
        "metadata": {
            "finalizers": finalizers
        }
    });

    api.patch(
        &instance.name_any(),
        &PatchParams::default(),
        &Patch::Merge(patch),
    )
    .await?;

    Ok(())
}

async fn update_status(
    instance: &Instance,
    api: &Api<Instance>,
    status: InstanceStatus,
) -> Result<()> {
    let patch = serde_json::json!({
        "status": status
    });

    api.patch_status(
        &instance.name_any(),
        &PatchParams::default(),
        &Patch::Merge(patch),
    )
    .await?;

    Ok(())
}

async fn validate_spec(
    spec: &crate::resources::InstanceSpec,
    scaleway_client: &ScalewayClient,
) -> Result<()> {
    scaleway_client.validate_zone(&spec.zone).await?;
    scaleway_client
        .validate_instance_type(&spec.instance_type)
        .await?;

    if spec.name.is_empty() {
        return Err(OperatorError::ConfigError(
            "name cannot be empty".to_string(),
        ));
    }

    Ok(())
}

/// Retourne true si l'erreur est permanente (spec/config incorrecte, ne pas requeue).
/// Fonction extraite pour être testable sans dépendance sur Arc<Instance> ou Arc<Context>.
fn is_permanent_error(error: &OperatorError) -> bool {
    matches!(
        error,
        OperatorError::InvalidZone(_)
            | OperatorError::InvalidInstanceType(_)
            | OperatorError::ConfigError(_)
            | OperatorError::ProjectAccessDenied(_)
    )
}

pub fn error_policy(_instance: Arc<Instance>, error: &OperatorError, _ctx: Arc<Context>) -> Action {
    if is_permanent_error(error) {
        tracing::warn!(error = %error, "Permanent configuration error — waiting for spec change");
        Action::await_change()
    } else {
        tracing::error!(error = %error, "Transient reconciliation error");
        Action::requeue(Duration::from_secs(60))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- role_allows_write ---

    #[test]
    fn test_editor_allows_write() {
        assert!(role_allows_write("Editor"));
    }

    #[test]
    fn test_admin_allows_write() {
        assert!(role_allows_write("Admin"));
    }

    #[test]
    fn test_organization_owner_allows_write() {
        assert!(role_allows_write("OrganizationOwner"));
    }

    #[test]
    fn test_viewer_does_not_allow_write() {
        assert!(!role_allows_write("Viewer"));
    }

    #[test]
    fn test_billing_viewer_does_not_allow_write() {
        assert!(!role_allows_write("BillingViewer"));
    }

    #[test]
    fn test_security_responsible_does_not_allow_write() {
        assert!(!role_allows_write("SecurityResponsible"));
    }

    #[test]
    fn test_unknown_role_does_not_allow_write() {
        assert!(!role_allows_write("UnknownRole"));
    }

    // --- is_permanent_error / error_policy classification ---

    #[test]
    fn test_config_error_is_permanent() {
        assert!(is_permanent_error(&OperatorError::ConfigError(
            "bad annotation".into()
        )));
    }

    #[test]
    fn test_invalid_zone_is_permanent() {
        assert!(is_permanent_error(&OperatorError::InvalidZone(
            "us-east-1".into()
        )));
    }

    #[test]
    fn test_invalid_instance_type_is_permanent() {
        assert!(is_permanent_error(&OperatorError::InvalidInstanceType(
            "MEGA-XL".into()
        )));
    }

    #[test]
    fn test_project_access_denied_is_permanent() {
        assert!(is_permanent_error(&OperatorError::ProjectAccessDenied(
            "proj-x".into()
        )));
    }

    #[test]
    fn test_scaleway_error_is_transient() {
        assert!(!is_permanent_error(&OperatorError::ScalewayError {
            status: "500 Internal Server Error".into(),
            message: "server error".into(),
        }));
    }

    #[test]
    fn test_kube_error_is_transient() {
        // KubeError wraps kube::error::Error — difficile à construire directement.
        // On vérifie via FinalizationError comme proxy d'erreur non-permanente.
        assert!(!is_permanent_error(&OperatorError::FinalizationError(
            "timeout".into()
        )));
    }

    #[test]
    fn test_unknown_error_is_transient() {
        assert!(!is_permanent_error(&OperatorError::Unknown(
            "mystery".into()
        )));
    }
}
