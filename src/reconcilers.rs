use crate::context::Context;
use crate::context::{extract_project_id_from_namespace, get_scaleway_role_for_namespace};
use crate::error::{OperatorError, Result};
use crate::metrics::{OperatorMetrics, ReconcileOutcome};
use crate::resources::{Instance, InstanceStatus};
use crate::scaleway::ScalewayClient;
use chrono::Utc;
use k8s_openapi::api::core::v1::Secret;
use kube::api::Patch;
use kube::runtime::controller::Action;
use kube::{api::PatchParams, Api, ResourceExt};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const INSTANCE_FINALIZER: &str = "scaleway.mathieubodin.io/instance-finalizer";
const NAMESPACE_CREDS_NS: &str = "scaleway-system";

// ── ReconcileMeasurer — RAII timer that records duration + outcome ────────────

struct ReconcileMeasurer<'a> {
    start: Instant,
    outcome: Option<ReconcileOutcome>,
    metrics: &'a OperatorMetrics,
    last_reconcile_at: &'a AtomicI64,
}

impl<'a> ReconcileMeasurer<'a> {
    fn new(metrics: &'a OperatorMetrics, last_reconcile_at: &'a AtomicI64) -> Self {
        Self {
            start: Instant::now(),
            outcome: None,
            metrics,
            last_reconcile_at,
        }
    }

    fn set_outcome(&mut self, o: ReconcileOutcome) {
        self.outcome = Some(o);
    }
}

impl Drop for ReconcileMeasurer<'_> {
    fn drop(&mut self) {
        let outcome = self.outcome.take().unwrap_or_else(|| {
            tracing::warn!("ReconcileMeasurer dropped without outcome set");
            ReconcileOutcome::Error
        });
        let duration_secs = self.start.elapsed().as_secs_f64();
        self.metrics.record_duration(&outcome, duration_secs);

        if outcome != ReconcileOutcome::Error {
            let now_secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            self.last_reconcile_at.store(now_secs, Ordering::Release);
        }
    }
}

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
        let mut measurer =
            ReconcileMeasurer::new(&ctx.metrics, &ctx.last_reconcile_at);
        return handle_deletion(&instance, &api, &ctx, &mut measurer).await;
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

    // Measurer is created here — after all early returns that don't represent a full
    // reconcile cycle (deletion_timestamp, missing prerequisites, finalizer add).
    let mut measurer = ReconcileMeasurer::new(&ctx.metrics, &ctx.last_reconcile_at);

    // 5. Valider la spec
    if let Err(e) = validate_spec(&instance.spec, &ctx.scaleway_client).await {
        measurer.set_outcome(ReconcileOutcome::Error);
        return Err(e);
    }

    // 6. Lire les credentials IAM pré-provisionnés pour ce namespace
    let ns_client = match get_namespace_client(&ctx, &namespace).await {
        Ok(client) => client,
        Err(e) => {
            tracing::error!(name = %instance.name_any(), namespace = %namespace, error = %e, "Missing pre-provisioned IAM credentials");
            let mut status = instance.status.clone().unwrap_or_default();
            status.error_message = Some(e.for_status());
            status.sync_state = "Error".to_string();
            let _ = update_status(&instance, &api, status).await;
            measurer.set_outcome(ReconcileOutcome::Error);
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
            measurer.set_outcome(ReconcileOutcome::Error);
            return Err(e);
        }

        // Vérifier l'accès projet uniquement à la première création (status.project_id absent).
        // Sauter lors d'une re-création après InstanceNotFound : le projet était déjà validé.
        if status.project_id.is_none() {
            ctx.scaleway_client
                .verify_project_access(&project_id)
                .await?;
        }

        // Cherche d'abord une instance existante par nom : récupère une instance
        // orpheline si le status n'a pas pu être écrit lors d'une réconciliation précédente.
        let (instance_id, adopted) = match ns_client
            .find_instance_by_name(&instance.spec.zone, &instance.spec.name, &project_id)
            .await?
        {
            Some(existing_id) => {
                tracing::warn!(
                    name = %instance.name_any(),
                    scaleway_id = %existing_id,
                    "Adopted existing Scaleway instance (status write may have failed previously)"
                );
                (existing_id, true)
            }
            None => {
                tracing::info!(name = %instance.name_any(), project_id = %project_id, "Creating new Scaleway instance");
                match ns_client.create_instance(&instance.spec, &project_id).await {
                    Ok(id) => (id, false),
                    Err(e) => {
                        tracing::error!(name = %instance.name_any(), error = %e, "Failed to create instance");
                        status.error_message = Some(e.for_status());
                        status.sync_state = "Error".to_string();
                        update_status(&instance, &api, status).await?;
                        measurer.set_outcome(ReconcileOutcome::Error);
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

        if adopted {
            measurer.set_outcome(ReconcileOutcome::Adopted);
        } else {
            measurer.set_outcome(ReconcileOutcome::Created);
        }
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
                measurer.set_outcome(ReconcileOutcome::Synced);
                update_status(&instance, &api, status).await?;
            }
            Err(OperatorError::InstanceNotFound(_)) => {
                tracing::warn!(name = %instance.name_any(), "Instance not found in Scaleway — will recreate");
                status.scaleway_id = None;
                status.state = "unknown".to_string();
                status.public_ip = None;
                status.project_id = None;
                status.created_at = None;
                status.error_message = None;
                status.sync_state = "Syncing".to_string();
                if let Err(patch_err) = update_status(&instance, &api, status).await {
                    tracing::warn!(error = %patch_err, "Failed to clear scaleway_id after NotFound — will retry");
                }
                // Requeue at 30s (not 5s) to allow Scaleway eventual consistency
                // to propagate before find_instance_by_name runs on the next cycle.
                // This prevents duplicate creation during short propagation windows.
                measurer.set_outcome(ReconcileOutcome::Error);
                return Ok(Action::requeue(Duration::from_secs(30)));
            }
            Err(e) => {
                tracing::warn!(name = %instance.name_any(), error = %e, "Failed to sync instance status");
                status.error_message = Some(e.for_status());
                status.sync_state = "Error".to_string();
                update_status(&instance, &api, status).await?;
                measurer.set_outcome(ReconcileOutcome::Error);
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
    measurer: &mut ReconcileMeasurer<'_>,
) -> std::result::Result<Action, OperatorError> {
    tracing::info!(
        name = %instance.name_any(),
        "Deleting instance"
    );

    if let Some(status) = &instance.status {
        if let Some(instance_id) = &status.scaleway_id {
            let namespace = instance.namespace().unwrap_or_default();
            match get_namespace_client(ctx, &namespace).await {
                Ok(ns_client) => {
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
                            measurer.set_outcome(ReconcileOutcome::Error);
                            return Err(e);
                        }
                    }
                }
                Err(e) => {
                    // IAM Secret absent : skip the Scaleway DELETE and proceed to
                    // finalizer removal. Permanently blocking deletion is worse than
                    // potentially leaving a cloud resource — the admin can clean up
                    // the Scaleway instance manually.
                    tracing::warn!(
                        name = %instance.name_any(),
                        instance_id = %instance_id,
                        error = %e,
                        "IAM Secret missing during deletion — skipping Scaleway API call, proceeding to finalizer removal"
                    );
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
    .await
    .map_err(|e| {
        measurer.set_outcome(ReconcileOutcome::Error);
        OperatorError::KubeError(e)
    })?;

    measurer.set_outcome(ReconcileOutcome::Deleted);
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

pub fn error_policy(_instance: Arc<Instance>, error: &OperatorError, ctx: Arc<Context>) -> Action {
    ctx.metrics.record_error(error);
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

    // ── ReconcileMeasurer unit tests ─────────────────────────────────────────

    fn fresh_metrics() -> OperatorMetrics {
        OperatorMetrics::new(&prometheus::Registry::new()).unwrap()
    }

    fn histogram_sample_count(metrics: &OperatorMetrics, outcome_label: &str) -> u64 {
        metrics.reconcile_duration_seconds
            .with_label_values(&[outcome_label])
            .get_sample_count()
    }

    /// Drop without set_outcome defaults to Error and records a duration observation.
    #[test]
    fn test_measurer_drop_without_outcome_defaults_to_error() {
        let metrics = fresh_metrics();
        let last_reconcile_at = AtomicI64::new(0);
        {
            let _measurer = ReconcileMeasurer::new(&metrics, &last_reconcile_at);
            // drop without set_outcome
        }
        // Duration should have been observed under the "Error" label
        assert_eq!(histogram_sample_count(&metrics, "Error"), 1,
            "Error histogram should have 1 observation after drop-without-outcome");
    }

    /// Drop without set_outcome must NOT update last_reconcile_at.
    #[test]
    fn test_measurer_drop_without_outcome_does_not_update_last_reconcile_at() {
        let metrics = fresh_metrics();
        let last_reconcile_at = AtomicI64::new(0);
        {
            let _measurer = ReconcileMeasurer::new(&metrics, &last_reconcile_at);
            // drop without set_outcome → defaults to Error
        }
        assert_eq!(
            last_reconcile_at.load(Ordering::Relaxed),
            0,
            "last_reconcile_at must NOT be updated when outcome is Error"
        );
    }

    /// set_outcome(Error) must NOT update last_reconcile_at.
    #[test]
    fn test_measurer_error_outcome_does_not_update_last_reconcile_at() {
        let metrics = fresh_metrics();
        let last_reconcile_at = AtomicI64::new(0);
        {
            let mut measurer = ReconcileMeasurer::new(&metrics, &last_reconcile_at);
            measurer.set_outcome(ReconcileOutcome::Error);
        }
        assert_eq!(
            last_reconcile_at.load(Ordering::Relaxed),
            0,
            "last_reconcile_at must NOT be updated when outcome is Error"
        );
    }

    /// set_outcome(Synced) MUST update last_reconcile_at to a recent timestamp.
    #[test]
    fn test_measurer_synced_outcome_updates_last_reconcile_at() {
        let metrics = fresh_metrics();
        let last_reconcile_at = AtomicI64::new(0);
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        {
            let mut measurer = ReconcileMeasurer::new(&metrics, &last_reconcile_at);
            measurer.set_outcome(ReconcileOutcome::Synced);
        }
        let after = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let stored = last_reconcile_at.load(Ordering::Relaxed);
        assert!(
            stored >= before && stored <= after,
            "last_reconcile_at should be a recent Unix timestamp, got {} (expected between {} and {})",
            stored, before, after
        );
        // Duration must also be observed under the Synced label
        assert_eq!(histogram_sample_count(&metrics, "Synced"), 1);
    }

    /// set_outcome(Created) MUST update last_reconcile_at.
    #[test]
    fn test_measurer_created_outcome_updates_last_reconcile_at() {
        let metrics = fresh_metrics();
        let last_reconcile_at = AtomicI64::new(0);
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        {
            let mut measurer = ReconcileMeasurer::new(&metrics, &last_reconcile_at);
            measurer.set_outcome(ReconcileOutcome::Created);
        }
        let stored = last_reconcile_at.load(Ordering::Relaxed);
        let after = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        assert!(
            stored >= before && stored <= after,
            "last_reconcile_at should be a recent Unix timestamp for Created outcome"
        );
    }

    /// set_outcome(Adopted) MUST update last_reconcile_at.
    #[test]
    fn test_measurer_adopted_outcome_updates_last_reconcile_at() {
        let metrics = fresh_metrics();
        let last_reconcile_at = AtomicI64::new(0);
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        {
            let mut measurer = ReconcileMeasurer::new(&metrics, &last_reconcile_at);
            measurer.set_outcome(ReconcileOutcome::Adopted);
        }
        let stored = last_reconcile_at.load(Ordering::Relaxed);
        let after = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        assert!(
            stored >= before && stored <= after,
            "last_reconcile_at should be a recent Unix timestamp for Adopted outcome"
        );
        assert_eq!(histogram_sample_count(&metrics, "Adopted"), 1);
    }

    /// set_outcome(Deleted) MUST update last_reconcile_at.
    #[test]
    fn test_measurer_deleted_outcome_updates_last_reconcile_at() {
        let metrics = fresh_metrics();
        let last_reconcile_at = AtomicI64::new(0);
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        {
            let mut measurer = ReconcileMeasurer::new(&metrics, &last_reconcile_at);
            measurer.set_outcome(ReconcileOutcome::Deleted);
        }
        let stored = last_reconcile_at.load(Ordering::Relaxed);
        let after = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        assert!(
            stored >= before && stored <= after,
            "last_reconcile_at should be a recent Unix timestamp for Deleted outcome"
        );
    }

    // ── error_policy unit tests ──────────────────────────────────────────────

    fn make_test_context() -> Arc<Context> {
        // Build a kube::Client from a dummy URL — no actual connection is made.
        let config = kube::Config::new(
            "http://localhost:0"
                .parse()
                .expect("dummy URL must be valid"),
        );
        let client = kube::Client::try_from(config).expect("Client from dummy config must succeed");
        Arc::new(Context {
            client,
            scaleway_client: crate::scaleway::ScalewayClient::new_with_base_url(
                "test-token".to_string(),
                "http://localhost:0".to_string(),
            ),
            organization_id: "test-org".to_string(),
            scaleway_base_url: "http://localhost:0".to_string(),
            metrics: fresh_metrics(),
            last_reconcile_at: AtomicI64::new(0),
        })
    }

    fn dummy_instance() -> Arc<Instance> {
        use crate::resources::{Instance, InstanceSpec};
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
        Arc::new(Instance {
            metadata: ObjectMeta {
                name: Some("test".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            spec: InstanceSpec {
                name: "test".to_string(),
                zone: "fr-par-1".to_string(),
                image: "ubuntu-jammy".to_string(),
                instance_type: "DEV1-S".to_string(),
                tags: vec![],
                boot_volume_size: 20,
                network: None,
                security: None,
            },
            status: None,
        })
    }

    /// error_policy with ConfigError increments the ConfigError counter.
    #[tokio::test]
    async fn test_error_policy_increments_config_error_counter() {
        let ctx = make_test_context();
        let err = OperatorError::ConfigError("bad annotation".to_string());
        error_policy(dummy_instance(), &err, ctx.clone());
        let value = ctx
            .metrics
            .reconcile_errors_total
            .with_label_values(&["ConfigError"])
            .get();
        assert_eq!(value, 1, "ConfigError counter should be 1");
        // Other labels must remain 0
        let other = ctx
            .metrics
            .reconcile_errors_total
            .with_label_values(&["NetworkError"])
            .get();
        assert_eq!(other, 0, "NetworkError counter must remain 0");
    }

    /// error_policy with a different error variant increments the correct label only.
    #[tokio::test]
    async fn test_error_policy_increments_unknown_error_counter() {
        let ctx = make_test_context();
        let err = OperatorError::Unknown("mystery".to_string());
        error_policy(dummy_instance(), &err, ctx.clone());
        let value = ctx
            .metrics
            .reconcile_errors_total
            .with_label_values(&["Unknown"])
            .get();
        assert_eq!(value, 1, "Unknown counter should be 1");
    }

    /// Calling error_policy twice with the same variant increments to 2.
    #[tokio::test]
    async fn test_error_policy_counter_accumulates() {
        let ctx = make_test_context();
        let err = OperatorError::ConfigError("x".to_string());
        error_policy(dummy_instance(), &err, ctx.clone());
        error_policy(dummy_instance(), &err, ctx.clone());
        let value = ctx
            .metrics
            .reconcile_errors_total
            .with_label_values(&["ConfigError"])
            .get();
        assert_eq!(value, 2);
    }
}
