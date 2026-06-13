use crate::context::Context;
use crate::context::{extract_project_id_from_namespace, get_scaleway_role_for_namespace};
use crate::error::{OperatorError, Result};
use crate::metrics::{OperatorMetrics, ReconcileOutcome};
use crate::resources::{
    Instance, InstanceStatus, LoadBalancer, LoadBalancerStatus, ScalewaySecret,
    ScalewaySecretStatus,
};
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
const LB_FINALIZER: &str = "scaleway.mathieubodin.io/loadbalancer-finalizer";
const SECRET_FINALIZER: &str = "scaleway.mathieubodin.io/secret-finalizer";
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

// ── ReconcileInput / ReconcileDecision — pure decision layer ─────────────────

/// Snapshot immuable des faits observables nécessaires à la décision de réconciliation.
/// Tout champ relatif à l'I/O (scaleway_role, project_id) est initialisé à String::new()
/// quand il n'est pas pertinent (suppression, circuit ouvert).
struct ReconcileInput {
    deletion_requested: bool,
    circuit_open: bool,
    finalizer_present: bool,
    scaleway_role: String,
    project_id: String,
    scaleway_id: Option<String>,
    status_project_id: Option<String>,
}

/// Décision pure dérivée d'un `ReconcileInput`, sans effet de bord.
#[derive(Debug)]
enum ReconcileDecision {
    /// Le circuit breaker est ouvert — ignorer cette réconciliation.
    SkipCircuitOpen,
    /// Ajouter le finalizer, puis requeue.
    AddFinalizer,
    /// Le rôle ne permet pas la création — bloquer avec erreur permanente.
    BlockReadOnlyRole,
    /// Vérifier l'accès projet, puis continuer directement vers la création.
    VerifyProjectAccess { project_id: String },
    /// Créer l'instance (accès projet déjà validé lors d'un cycle précédent).
    CreateInstance { project_id: String },
    /// Synchroniser l'état depuis Scaleway.
    SyncInstance {
        scaleway_id: String,
        project_id: String,
    },
    /// Supprimer l'instance Scaleway puis retirer le finalizer.
    DeleteInstance,
    /// Retirer le finalizer (aucune instance Scaleway connue).
    RemoveFinalizer,
}

/// Dérive la prochaine action à effectuer à partir d'un snapshot d'état, sans effet de bord.
fn decide_next_action(input: &ReconcileInput) -> ReconcileDecision {
    // 1. Suppression prioritaire — avant toute autre vérification
    if input.deletion_requested {
        return match &input.scaleway_id {
            Some(_) => ReconcileDecision::DeleteInstance,
            None => ReconcileDecision::RemoveFinalizer,
        };
    }

    // 2. Circuit breaker
    if input.circuit_open {
        return ReconcileDecision::SkipCircuitOpen;
    }

    // 3. Finalizer absent — l'ajouter avant tout
    if !input.finalizer_present {
        return ReconcileDecision::AddFinalizer;
    }

    // 4. Instance déjà connue — synchroniser
    if let Some(scaleway_id) = &input.scaleway_id {
        return ReconcileDecision::SyncInstance {
            scaleway_id: scaleway_id.clone(),
            project_id: input.project_id.clone(),
        };
    }

    // 5. Création — vérifier le rôle
    if !role_allows_write(&input.scaleway_role) {
        return ReconcileDecision::BlockReadOnlyRole;
    }

    // 6. Vérification accès projet seulement à la première création
    if input.status_project_id.is_none() {
        return ReconcileDecision::VerifyProjectAccess {
            project_id: input.project_id.clone(),
        };
    }

    // 7. Accès projet déjà validé — créer directement
    ReconcileDecision::CreateInstance {
        project_id: input.project_id.clone(),
    }
}

/// Retourne true si le rôle autorise les opérations d'écriture sur les instances.
fn role_allows_write(role: &str) -> bool {
    matches!(role, "Editor" | "Admin" | "OrganizationOwner")
}

// ── LbReconcileInput / LbReconcileDecision — couche de décision pure LoadBalancer ──

struct LbReconcileInput {
    deletion_requested: bool,
    circuit_open: bool,
    finalizer_present: bool,
    scaleway_role: String,
    project_id: String,
    scaleway_id: Option<String>,
    status_project_id: Option<String>,
}

#[derive(Debug)]
enum LbReconcileDecision {
    SkipCircuitOpen,
    AddLbFinalizer,
    BlockReadOnlyRole,
    VerifyProjectAccessLb {
        project_id: String,
    },
    CreateLoadBalancer {
        project_id: String,
    },
    SyncLoadBalancer {
        scaleway_id: String,
        project_id: String,
    },
    DeleteLoadBalancer,
    RemoveLbFinalizer,
}

fn decide_next_action_lb(input: &LbReconcileInput) -> LbReconcileDecision {
    // 1. Suppression prioritaire — avant toute autre vérification
    if input.deletion_requested {
        return match &input.scaleway_id {
            Some(_) => LbReconcileDecision::DeleteLoadBalancer,
            None => LbReconcileDecision::RemoveLbFinalizer,
        };
    }

    // 2. Circuit breaker
    if input.circuit_open {
        return LbReconcileDecision::SkipCircuitOpen;
    }

    // 3. Finalizer absent — l'ajouter avant tout
    if !input.finalizer_present {
        return LbReconcileDecision::AddLbFinalizer;
    }

    // 4. LB déjà connu — synchroniser
    if let Some(scaleway_id) = &input.scaleway_id {
        return LbReconcileDecision::SyncLoadBalancer {
            scaleway_id: scaleway_id.clone(),
            project_id: input.project_id.clone(),
        };
    }

    // 5. Création — vérifier le rôle
    if !role_allows_write(&input.scaleway_role) {
        return LbReconcileDecision::BlockReadOnlyRole;
    }

    // 6. Vérification accès projet seulement à la première création
    if input.status_project_id.is_none() {
        return LbReconcileDecision::VerifyProjectAccessLb {
            project_id: input.project_id.clone(),
        };
    }

    // 7. Accès projet déjà validé — créer directement
    LbReconcileDecision::CreateLoadBalancer {
        project_id: input.project_id.clone(),
    }
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

/// Récupérer le project_id depuis l'annotation du namespace pour n'importe quelle ressource.
async fn get_project_id_from_namespace_resource(
    resource: &impl kube::ResourceExt,
    ctx: &Arc<Context>,
) -> Result<String> {
    let namespace = resource.namespace().unwrap_or_default();
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
    let key = format!(
        "instance/{}/{}",
        instance.namespace().unwrap_or_default(),
        instance.name_any()
    );
    let result = reconcile_instance_inner(instance, ctx.clone()).await;
    if result.is_ok() {
        ctx.reset_retry_count(&key);
    }
    result
}

async fn reconcile_instance_inner(
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

    // ── Collecte des inputs (Option A : collecte conditionnelle) ──────────────

    let deletion_requested = instance.metadata.deletion_timestamp.is_some();
    let circuit_open = ctx.is_circuit_open();
    let finalizer_present = instance
        .metadata
        .finalizers
        .as_ref()
        .unwrap_or(&vec![])
        .contains(&INSTANCE_FINALIZER.to_string());

    let (scaleway_role, project_id) = if !deletion_requested && !circuit_open {
        // Obtenir le rôle Scaleway depuis la ressource NamespaceRole
        let role = match get_scaleway_role_for_namespace(&ctx.client, &namespace).await {
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

        // Obtenir le project_id depuis l'annotation du namespace
        let pid = match get_project_id_from_namespace_resource(instance.as_ref(), &ctx).await {
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
            role = %role,
            "Using Scaleway role for namespace"
        );

        (role, pid)
    } else {
        // Ignorés par decide_next_action dans les cas suppression/circuit ouvert
        (String::new(), String::new())
    };

    let current_status = instance.status.clone().unwrap_or_default();

    let input = ReconcileInput {
        deletion_requested,
        circuit_open,
        finalizer_present,
        scaleway_role: scaleway_role.clone(),
        project_id: project_id.clone(),
        scaleway_id: current_status.scaleway_id.clone(),
        status_project_id: current_status.project_id.clone(),
    };

    // ── Décision pure ─────────────────────────────────────────────────────────

    let decision = decide_next_action(&input);

    // ── Exécution de l'I/O correspondante ────────────────────────────────────

    match decision {
        ReconcileDecision::SkipCircuitOpen => {
            tracing::warn!(
                name = %instance.name_any(),
                namespace = %namespace,
                "Scaleway API circuit breaker is open — skipping reconciliation"
            );
            Err(OperatorError::CircuitBreakerOpen)
        }

        ReconcileDecision::AddFinalizer => {
            add_finalizer(&instance, &api).await?;
            Ok(Action::requeue(Duration::from_secs(5)))
        }

        ReconcileDecision::BlockReadOnlyRole => {
            let mut measurer = ReconcileMeasurer::new(&ctx.metrics, &ctx.last_reconcile_at);
            let e = OperatorError::ConfigError(format!(
                "Role '{}' is read-only and cannot create instances. Use 'Editor' or 'Admin'.",
                input.scaleway_role
            ));
            let mut status = instance.status.clone().unwrap_or_default();
            status.error_message = Some(e.for_status());
            status.sync_state = "Error".to_string();
            let _ = update_status(&instance, &api, status).await;
            measurer.set_outcome(ReconcileOutcome::Error);
            Err(e)
        }

        ReconcileDecision::VerifyProjectAccess { project_id } => {
            let mut measurer = ReconcileMeasurer::new(&ctx.metrics, &ctx.last_reconcile_at);

            // Valider la spec
            if let Err(e) = validate_spec(&instance.spec, &ctx.scaleway_client).await {
                measurer.set_outcome(ReconcileOutcome::Error);
                return Err(e);
            }

            // Lire les credentials IAM pré-provisionnés pour ce namespace
            let ns_client = match get_namespace_client(&ctx, &namespace).await {
                Ok(client) => client,
                Err(e) => {
                    tracing::error!(name = %instance.name_any(), namespace = %namespace, error = %e, "Missing pre-provisioned IAM credentials");
                    let mut st = instance.status.clone().unwrap_or_default();
                    st.error_message = Some(e.for_status());
                    st.sync_state = "Error".to_string();
                    let _ = update_status(&instance, &api, st).await;
                    measurer.set_outcome(ReconcileOutcome::Error);
                    return Err(e);
                }
            };

            // Vérifier l'accès projet
            call_scaleway(&ctx, || {
                ctx.scaleway_client.verify_project_access(&project_id)
            })
            .await?;

            // Continuer directement vers la création dans le même cycle
            execute_create_instance(
                &instance,
                &api,
                &ctx,
                &namespace,
                &ns_client,
                &project_id,
                &mut measurer,
            )
            .await
        }

        ReconcileDecision::CreateInstance { project_id } => {
            let mut measurer = ReconcileMeasurer::new(&ctx.metrics, &ctx.last_reconcile_at);

            // Valider la spec
            if let Err(e) = validate_spec(&instance.spec, &ctx.scaleway_client).await {
                measurer.set_outcome(ReconcileOutcome::Error);
                return Err(e);
            }

            // Lire les credentials IAM pré-provisionnés pour ce namespace
            let ns_client = match get_namespace_client(&ctx, &namespace).await {
                Ok(client) => client,
                Err(e) => {
                    tracing::error!(name = %instance.name_any(), namespace = %namespace, error = %e, "Missing pre-provisioned IAM credentials");
                    let mut st = instance.status.clone().unwrap_or_default();
                    st.error_message = Some(e.for_status());
                    st.sync_state = "Error".to_string();
                    let _ = update_status(&instance, &api, st).await;
                    measurer.set_outcome(ReconcileOutcome::Error);
                    return Err(e);
                }
            };

            execute_create_instance(
                &instance,
                &api,
                &ctx,
                &namespace,
                &ns_client,
                &project_id,
                &mut measurer,
            )
            .await
        }

        ReconcileDecision::SyncInstance {
            scaleway_id,
            project_id,
        } => {
            let mut measurer = ReconcileMeasurer::new(&ctx.metrics, &ctx.last_reconcile_at);

            // Valider la spec
            if let Err(e) = validate_spec(&instance.spec, &ctx.scaleway_client).await {
                measurer.set_outcome(ReconcileOutcome::Error);
                return Err(e);
            }

            // Lire les credentials IAM pré-provisionnés pour ce namespace
            let ns_client = match get_namespace_client(&ctx, &namespace).await {
                Ok(client) => client,
                Err(e) => {
                    tracing::error!(name = %instance.name_any(), namespace = %namespace, error = %e, "Missing pre-provisioned IAM credentials");
                    let mut st = instance.status.clone().unwrap_or_default();
                    st.error_message = Some(e.for_status());
                    st.sync_state = "Error".to_string();
                    let _ = update_status(&instance, &api, st).await;
                    measurer.set_outcome(ReconcileOutcome::Error);
                    return Err(e);
                }
            };

            let mut status = instance.status.clone().unwrap_or_default();

            match call_scaleway(&ctx, || {
                ns_client.get_instance(&instance.spec.zone, &scaleway_id, &project_id)
            })
            .await
            {
                Ok(info) => {
                    // Gauge swap: dec old state, inc new state
                    let old_state = instance
                        .status
                        .as_ref()
                        .map(|s| s.state.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    ctx.metrics.dec_instances(
                        &instance.spec.zone,
                        &instance.spec.instance_type,
                        &old_state,
                    );
                    ctx.metrics.inc_instances(
                        &instance.spec.zone,
                        &instance.spec.instance_type,
                        &info.state,
                    );

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
                    // Decrement gauge only if the instance was previously created (scaleway_id present)
                    if instance
                        .status
                        .as_ref()
                        .and_then(|s| s.scaleway_id.as_ref())
                        .is_some()
                    {
                        let old_state = instance
                            .status
                            .as_ref()
                            .map(|s| s.state.as_str())
                            .unwrap_or("unknown");
                        ctx.metrics.dec_instances(
                            &instance.spec.zone,
                            &instance.spec.instance_type,
                            old_state,
                        );
                    }
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

            Ok(Action::requeue(Duration::from_secs(30)))
        }

        ReconcileDecision::DeleteInstance | ReconcileDecision::RemoveFinalizer => {
            handle_deletion(&instance, &api, &ctx).await
        }
    }
}

/// Logique de création partagée entre VerifyProjectAccess et CreateInstance.
/// Le measurer doit avoir été créé par l'appelant.
async fn execute_create_instance(
    instance: &Instance,
    api: &Api<Instance>,
    ctx: &Arc<Context>,
    _namespace: &str,
    ns_client: &ScalewayClient,
    project_id: &str,
    measurer: &mut ReconcileMeasurer<'_>,
) -> std::result::Result<Action, OperatorError> {
    let mut status = instance.status.clone().unwrap_or_default();

    // Cherche d'abord une instance existante par nom : récupère une instance
    // orpheline si le status n'a pas pu être écrit lors d'une réconciliation précédente.
    let (instance_id, adopted) = match ns_client
        .find_instance_by_name(&instance.spec.zone, &instance.spec.name, project_id)
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
            match call_scaleway(ctx, || {
                ns_client.create_instance(&instance.spec, project_id)
            })
            .await
            {
                Ok(id) => (id, false),
                Err(e) => {
                    tracing::error!(name = %instance.name_any(), error = %e, "Failed to create instance");
                    status.error_message = Some(e.for_status());
                    status.sync_state = "Error".to_string();
                    update_status(instance, api, status).await?;
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
    status.project_id = Some(project_id.to_string());

    ctx.metrics.inc_instances(
        &instance.spec.zone,
        &instance.spec.instance_type,
        "creating",
    );

    if adopted {
        measurer.set_outcome(ReconcileOutcome::Adopted);
    } else {
        measurer.set_outcome(ReconcileOutcome::Created);
    }
    update_status(instance, api, status.clone()).await?;
    Ok(Action::requeue(Duration::from_secs(10)))
}

/// Wraps a Scaleway API call to update the circuit breaker state.
/// On success: calls record_scaleway_success().
/// On transient error: calls record_scaleway_failure().
/// On permanent error: does not affect the circuit (permanent errors are spec/config issues).
async fn call_scaleway<T, F, Fut>(ctx: &Arc<Context>, f: F) -> Result<T>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let result = f().await;
    match &result {
        Ok(_) => ctx.record_scaleway_success(),
        Err(e) if !e.is_permanent_error() => ctx.record_scaleway_failure(),
        _ => {}
    }
    result
}

async fn handle_deletion(
    instance: &Instance,
    api: &Api<Instance>,
    ctx: &Arc<Context>,
) -> std::result::Result<Action, OperatorError> {
    let mut measurer = ReconcileMeasurer::new(&ctx.metrics, &ctx.last_reconcile_at);

    tracing::info!(
        name = %instance.name_any(),
        "Deleting instance"
    );

    if let Some(status) = &instance.status {
        if let Some(instance_id) = &status.scaleway_id {
            let namespace = instance.namespace().unwrap_or_default();
            match get_namespace_client(ctx, &namespace).await {
                Ok(ns_client) => {
                    match call_scaleway(ctx, || {
                        ns_client.delete_instance(&instance.spec.zone, instance_id)
                    })
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

    // Decrement gauge: the instance is no longer managed
    let old_state = instance
        .status
        .as_ref()
        .map(|s| s.state.as_str())
        .unwrap_or("unknown");
    ctx.metrics
        .dec_instances(&instance.spec.zone, &instance.spec.instance_type, old_state);

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
    scaleway_client.validate_instance_type(&spec.instance_type)?;

    if spec.name.is_empty() {
        return Err(OperatorError::ConfigError(
            "name cannot be empty".to_string(),
        ));
    }

    Ok(())
}

// ── LoadBalancer reconciler ───────────────────────────────────────────────────

pub async fn reconcile_load_balancer(
    lb: Arc<LoadBalancer>,
    ctx: Arc<Context>,
) -> std::result::Result<Action, OperatorError> {
    let key = format!(
        "loadbalancer/{}/{}",
        lb.namespace().unwrap_or_default(),
        lb.name_any()
    );
    let result = reconcile_load_balancer_inner(lb, ctx.clone()).await;
    if result.is_ok() {
        ctx.reset_retry_count(&key);
    }
    result
}

async fn reconcile_load_balancer_inner(
    lb: Arc<LoadBalancer>,
    ctx: Arc<Context>,
) -> std::result::Result<Action, OperatorError> {
    let namespace = lb.namespace().unwrap_or_default();
    let api: Api<LoadBalancer> = Api::namespaced(ctx.client.clone(), &namespace);

    tracing::info!(
        name = %lb.name_any(),
        namespace = %namespace,
        "Reconciling load balancer"
    );

    let deletion_requested = lb.metadata.deletion_timestamp.is_some();
    let circuit_open = ctx.is_circuit_open();
    let finalizer_present = lb
        .metadata
        .finalizers
        .as_ref()
        .unwrap_or(&vec![])
        .contains(&LB_FINALIZER.to_string());

    let (scaleway_role, project_id) = if !deletion_requested && !circuit_open {
        let role = match get_scaleway_role_for_namespace(&ctx.client, &namespace).await {
            Ok(role) => role,
            Err(e) => {
                tracing::error!(name = %lb.name_any(), namespace = %namespace, error = %e, "Cannot proceed without NamespaceRole");
                let mut status = lb.status.clone().unwrap_or_default();
                status.error_message = Some(e.for_status());
                status.sync_state = "Error".to_string();
                let _ = update_lb_status(&lb, &api, status).await;
                return Err(e);
            }
        };

        let pid = match get_project_id_from_namespace_resource(lb.as_ref(), &ctx).await {
            Ok(pid) => {
                if uuid::Uuid::parse_str(&pid).is_err() {
                    let e = OperatorError::ConfigError(format!(
                        "Annotation 'scaleway.mathieubodin.io/project-id' must be a valid UUID, got: '{}'",
                        pid
                    ));
                    let mut status = lb.status.clone().unwrap_or_default();
                    status.error_message = Some(e.for_status());
                    status.sync_state = "Error".to_string();
                    let _ = update_lb_status(&lb, &api, status).await;
                    return Err(e);
                }
                pid
            }
            Err(e) => {
                tracing::error!(name = %lb.name_any(), error = %e, "Cannot proceed without project_id from namespace annotation");
                let mut status = lb.status.clone().unwrap_or_default();
                status.error_message = Some(e.for_status());
                status.sync_state = "Error".to_string();
                let _ = update_lb_status(&lb, &api, status).await;
                return Err(e);
            }
        };

        (role, pid)
    } else {
        (String::new(), String::new())
    };

    let current_status = lb.status.clone().unwrap_or_default();

    let input = LbReconcileInput {
        deletion_requested,
        circuit_open,
        finalizer_present,
        scaleway_role: scaleway_role.clone(),
        project_id: project_id.clone(),
        scaleway_id: current_status.scaleway_id.clone(),
        status_project_id: current_status.project_id.clone(),
    };

    let decision = decide_next_action_lb(&input);

    match decision {
        LbReconcileDecision::SkipCircuitOpen => {
            tracing::warn!(name = %lb.name_any(), namespace = %namespace, "Scaleway API circuit breaker is open — skipping LB reconciliation");
            Err(OperatorError::CircuitBreakerOpen)
        }

        LbReconcileDecision::AddLbFinalizer => {
            add_lb_finalizer(&lb, &api).await?;
            Ok(Action::requeue(Duration::from_secs(5)))
        }

        LbReconcileDecision::BlockReadOnlyRole => {
            let mut measurer = ReconcileMeasurer::new(&ctx.metrics, &ctx.last_reconcile_at);
            let e = OperatorError::ConfigError(format!(
                "Role '{}' is read-only and cannot create load balancers. Use 'Editor' or 'Admin'.",
                input.scaleway_role
            ));
            let mut status = lb.status.clone().unwrap_or_default();
            status.error_message = Some(e.for_status());
            status.sync_state = "Error".to_string();
            let _ = update_lb_status(&lb, &api, status).await;
            measurer.set_outcome(ReconcileOutcome::Error);
            Err(e)
        }

        LbReconcileDecision::VerifyProjectAccessLb { project_id } => {
            let mut measurer = ReconcileMeasurer::new(&ctx.metrics, &ctx.last_reconcile_at);

            if let Err(e) = validate_lb_spec(&lb.spec, &ctx.scaleway_client).await {
                measurer.set_outcome(ReconcileOutcome::Error);
                return Err(e);
            }

            call_scaleway(&ctx, || {
                ctx.scaleway_client.verify_project_access(&project_id)
            })
            .await?;

            execute_create_load_balancer(&lb, &api, &ctx, &namespace, &project_id, &mut measurer)
                .await
        }

        LbReconcileDecision::CreateLoadBalancer { project_id } => {
            let mut measurer = ReconcileMeasurer::new(&ctx.metrics, &ctx.last_reconcile_at);

            if let Err(e) = validate_lb_spec(&lb.spec, &ctx.scaleway_client).await {
                measurer.set_outcome(ReconcileOutcome::Error);
                return Err(e);
            }

            execute_create_load_balancer(&lb, &api, &ctx, &namespace, &project_id, &mut measurer)
                .await
        }

        LbReconcileDecision::SyncLoadBalancer {
            scaleway_id,
            project_id,
        } => {
            let mut measurer = ReconcileMeasurer::new(&ctx.metrics, &ctx.last_reconcile_at);
            // GET uses the global operator token (read-only scope).
            // TODO: migrate to ns_client when the namespace IAM key covers read operations.

            let mut status = lb.status.clone().unwrap_or_default();

            match call_scaleway(&ctx, || {
                ctx.scaleway_client
                    .get_load_balancer(&lb.spec.zone, &scaleway_id)
            })
            .await
            {
                Ok(info) => {
                    let old_state = lb
                        .status
                        .as_ref()
                        .map(|s| s.state.as_str())
                        .unwrap_or("")
                        .to_string();
                    ctx.metrics
                        .dec_load_balancers(&lb.spec.zone, &lb.spec.lb_type, &old_state);
                    ctx.metrics
                        .inc_load_balancers(&lb.spec.zone, &lb.spec.lb_type, &info.state);

                    status.state = info.state;
                    status.vip_address = info.vip_address;
                    status.project_id = Some(project_id);
                    status.sync_state = "Synced".to_string();
                    status.error_message = None;
                    measurer.set_outcome(ReconcileOutcome::Synced);
                    update_lb_status(&lb, &api, status).await?;
                }
                Err(OperatorError::LbNotFound(_)) => {
                    tracing::warn!(name = %lb.name_any(), "Load balancer not found in Scaleway — will recreate");
                    if lb
                        .status
                        .as_ref()
                        .and_then(|s| s.scaleway_id.as_ref())
                        .is_some()
                    {
                        let old_state = lb.status.as_ref().map(|s| s.state.as_str()).unwrap_or("");
                        ctx.metrics
                            .dec_load_balancers(&lb.spec.zone, &lb.spec.lb_type, old_state);
                    }
                    status.scaleway_id = None;
                    status.state = String::new();
                    status.vip_address = None;
                    status.project_id = None;
                    status.error_message = None;
                    status.sync_state = "Pending".to_string();
                    if let Err(patch_err) = update_lb_status(&lb, &api, status).await {
                        tracing::warn!(error = %patch_err, "Failed to clear scaleway_id after LbNotFound");
                    }
                    measurer.set_outcome(ReconcileOutcome::Error);
                    return Ok(Action::requeue(Duration::from_secs(30)));
                }
                Err(e) => {
                    tracing::warn!(name = %lb.name_any(), error = %e, "Failed to sync load balancer status");
                    status.error_message = Some(e.for_status());
                    status.sync_state = "Error".to_string();
                    update_lb_status(&lb, &api, status).await?;
                    measurer.set_outcome(ReconcileOutcome::Error);
                    return Err(e);
                }
            }

            Ok(Action::requeue(Duration::from_secs(30)))
        }

        LbReconcileDecision::DeleteLoadBalancer | LbReconcileDecision::RemoveLbFinalizer => {
            handle_lb_deletion(&lb, &api, &ctx).await
        }
    }
}

async fn execute_create_load_balancer(
    lb: &LoadBalancer,
    api: &Api<LoadBalancer>,
    ctx: &Arc<Context>,
    namespace: &str,
    project_id: &str,
    measurer: &mut ReconcileMeasurer<'_>,
) -> std::result::Result<Action, OperatorError> {
    let mut status = lb.status.clone().unwrap_or_default();
    let cr_name = lb.name_any();

    // Orphan adoption via tag-based lookup (name is not unique in Scaleway LB API)
    let (lb_id, adopted) = match call_scaleway(ctx, || {
        ctx.scaleway_client.find_load_balancer_by_name(
            &lb.spec.zone,
            namespace,
            &cr_name,
            project_id,
        )
    })
    .await?
    {
        Some(existing_id) => {
            tracing::warn!(
                name = %lb.name_any(),
                scaleway_id = %existing_id,
                "Adopted existing Scaleway load balancer (status write may have failed previously)"
            );
            (existing_id, true)
        }
        None => {
            tracing::info!(name = %lb.name_any(), project_id = %project_id, "Creating new Scaleway load balancer");
            match call_scaleway(ctx, || {
                ctx.scaleway_client
                    .create_load_balancer(&lb.spec, project_id, namespace, &cr_name)
            })
            .await
            {
                Ok(id) => (id, false),
                Err(e) => {
                    tracing::error!(name = %lb.name_any(), error = %e, "Failed to create load balancer");
                    status.error_message = Some(e.for_status());
                    status.sync_state = "Error".to_string();
                    update_lb_status(lb, api, status).await?;
                    measurer.set_outcome(ReconcileOutcome::Error);
                    return Err(e);
                }
            }
        }
    };

    status.scaleway_id = Some(lb_id);
    status.state = "pending".to_string();
    status.sync_state = "Syncing".to_string();
    status.error_message = None;
    status.project_id = Some(project_id.to_string());

    ctx.metrics
        .inc_load_balancers(&lb.spec.zone, &lb.spec.lb_type, "pending");

    if adopted {
        measurer.set_outcome(ReconcileOutcome::Adopted);
    } else {
        measurer.set_outcome(ReconcileOutcome::Created);
    }
    update_lb_status(lb, api, status).await?;
    Ok(Action::requeue(Duration::from_secs(10)))
}

async fn handle_lb_deletion(
    lb: &LoadBalancer,
    api: &Api<LoadBalancer>,
    ctx: &Arc<Context>,
) -> std::result::Result<Action, OperatorError> {
    let mut measurer = ReconcileMeasurer::new(&ctx.metrics, &ctx.last_reconcile_at);

    tracing::info!(name = %lb.name_any(), "Deleting load balancer");

    if let Some(status) = &lb.status {
        if let Some(lb_id) = &status.scaleway_id {
            let namespace = lb.namespace().unwrap_or_default();
            match get_namespace_client(ctx, &namespace).await {
                Ok(_ns_client) => {
                    match call_scaleway(ctx, || {
                        ctx.scaleway_client
                            .delete_load_balancer(&lb.spec.zone, lb_id, true)
                    })
                    .await
                    {
                        Ok(_) => {
                            tracing::info!(name = %lb.name_any(), lb_id = %lb_id, "Successfully deleted Scaleway load balancer");
                        }
                        Err(OperatorError::ScalewayError { ref status, .. })
                            if status.contains("409") || status.contains("423") =>
                        {
                            tracing::warn!(name = %lb.name_any(), lb_id = %lb_id, "Load balancer is locked — cannot delete yet");
                            let mut st = lb.status.clone().unwrap_or_default();
                            st.sync_state = "TerminationBlocked".to_string();
                            st.error_message = Some(
                                "Load balancer is locked — deletion blocked by Scaleway"
                                    .to_string(),
                            );
                            let _ = update_lb_status(lb, api, st).await;
                            measurer.set_outcome(ReconcileOutcome::Error);
                            return Err(OperatorError::ScalewayError {
                                status: "409".to_string(),
                                message: "Load balancer is locked".to_string(),
                            });
                        }
                        Err(e) => {
                            tracing::error!(name = %lb.name_any(), error = %e, "Failed to delete Scaleway load balancer");
                            measurer.set_outcome(ReconcileOutcome::Error);
                            return Err(e);
                        }
                    }
                }
                Err(e) => {
                    // IAM Secret absent — write audit trail before removing finalizer
                    tracing::warn!(
                        name = %lb.name_any(),
                        lb_id = %lb_id,
                        error = %e,
                        "IAM Secret missing during LB deletion — skipping Scaleway API call"
                    );
                    let mut st = lb.status.clone().unwrap_or_default();
                    st.sync_state = "FinalizerRemovedWithoutScalewayDelete".to_string();
                    st.error_message = Some(
                        "IAM Secret missing at deletion time — Scaleway LB may still exist"
                            .to_string(),
                    );
                    if let Err(patch_err) = update_lb_status(lb, api, st).await {
                        tracing::error!(
                            name = %lb.name_any(),
                            lb_id = %lb_id,
                            error = %patch_err,
                            "Failed to write FinalizerRemovedWithoutScalewayDelete audit status — potential LB orphan in Scaleway"
                        );
                    }
                }
            }
        }
    }

    // Remove finalizer
    let finalizers = lb.metadata.finalizers.clone().unwrap_or_default();
    let new_finalizers: Vec<String> = finalizers
        .into_iter()
        .filter(|f| f != LB_FINALIZER)
        .collect();

    let patch = serde_json::json!({"metadata": {"finalizers": new_finalizers}});
    api.patch(
        &lb.name_any(),
        &PatchParams::default(),
        &Patch::Merge(patch),
    )
    .await
    .map_err(|e| {
        measurer.set_outcome(ReconcileOutcome::Error);
        OperatorError::KubeError(e)
    })?;

    let old_state = lb.status.as_ref().map(|s| s.state.as_str()).unwrap_or("");
    ctx.metrics
        .dec_load_balancers(&lb.spec.zone, &lb.spec.lb_type, old_state);

    measurer.set_outcome(ReconcileOutcome::Deleted);
    Ok(Action::await_change())
}

async fn add_lb_finalizer(lb: &LoadBalancer, api: &Api<LoadBalancer>) -> Result<()> {
    let mut finalizers = lb.metadata.finalizers.clone().unwrap_or_default();
    finalizers.push(LB_FINALIZER.to_string());
    let patch = serde_json::json!({"metadata": {"finalizers": finalizers}});
    api.patch(
        &lb.name_any(),
        &PatchParams::default(),
        &Patch::Merge(patch),
    )
    .await?;
    Ok(())
}

async fn update_lb_status(
    lb: &LoadBalancer,
    api: &Api<LoadBalancer>,
    status: LoadBalancerStatus,
) -> Result<()> {
    let patch = serde_json::json!({"status": status});
    api.patch_status(
        &lb.name_any(),
        &PatchParams::default(),
        &Patch::Merge(patch),
    )
    .await?;
    Ok(())
}

async fn validate_lb_spec(
    spec: &crate::resources::LoadBalancerSpec,
    scaleway_client: &ScalewayClient,
) -> Result<()> {
    scaleway_client.validate_zone(&spec.zone).await?;
    scaleway_client.validate_lb_type(&spec.lb_type)?;

    if spec.name.is_empty() {
        return Err(OperatorError::ConfigError(
            "name cannot be empty".to_string(),
        ));
    }

    Ok(())
}

// ── ScalewaySecret reconciler ────────────────────────────────────────────────

const OPT_IN_LABEL: &str = "scaleway.mathieubodin.io/allow-operator-read";
const ALLOWED_CR_ANNOTATION: &str = "scaleway.mathieubodin.io/allowed-cr";
const STATUS_ERROR_GENERIC: &str = "Source Secret unavailable (see operator logs)";

/// État lu en UNE seule fois depuis le Secret K8s source.
/// Évite la TOCTOU entre la vérification opt-in et la lecture de la valeur.
#[derive(Debug)]
struct KsSourceState {
    resource_version: Option<String>,
    key_present: bool,
    /// Charge utile décodée à partir de `.data[key]` quand `key_present` est true.
    payload: Option<Vec<u8>>,
}

/// Vérifie le label d'opt-in `scaleway.mathieubodin.io/allow-operator-read: "true"`.
/// Contrat strict : seule la chaîne exacte `"true"` autorise la lecture ; toute autre
/// valeur (absente, vide, `"yes"`, `"True"`) refuse l'opt-in. Fonction pure pour
/// pouvoir verrouiller ce contrat par test unitaire.
fn is_opt_in_granted(labels: Option<&std::collections::BTreeMap<String, String>>) -> bool {
    labels
        .and_then(|l| l.get(OPT_IN_LABEL))
        .map(|v| v == "true")
        .unwrap_or(false)
}

/// Vérifie que l'annotation `allowed-cr` du Secret K8s correspond à
/// `<cr_namespace>/<cr_name>`. Ferme le scénario où un utilisateur avec
/// `patch secrets` (mais sans `get secrets`) pourrait labelliser un Secret
/// qu'il ne possède pas. La cible est une chaîne strictement égale —
/// pas de wildcard, pas de liste — pour ne lier qu'à UN CR donné.
fn is_cr_allowed_for_secret(
    annotations: Option<&std::collections::BTreeMap<String, String>>,
    cr_namespace: &str,
    cr_name: &str,
) -> bool {
    let expected = format!("{}/{}", cr_namespace, cr_name);
    annotations
        .and_then(|a| a.get(ALLOWED_CR_ANNOTATION))
        .map(|v| v == &expected)
        .unwrap_or(false)
}

/// Mappe l'erreur d'un `get` sur l'API K8s vers une erreur métier explicite.
/// Fonction pure — testable unitairement avec des `kube::error::Error::Api` fabriqués.
///
/// - 403 → `ConfigError` permanent (l'opérateur n'a pas le droit de lire les Secrets dans ce
///   namespace ; le namespace n'est probablement pas bootstrappé)
/// - 404 → `SecretNotFound` transitoire (le Secret peut être créé plus tard)
/// - autre → `KubeError` transitoire
fn map_kube_get_error(e: kube::error::Error, ks_name: &str, namespace: &str) -> OperatorError {
    match e {
        kube::error::Error::Api(ae) if ae.code == 403 => OperatorError::ConfigError(format!(
            "Operator forbidden to read Secrets in namespace '{}' \
             (RBAC denied — verify the namespace is bootstrapped)",
            namespace
        )),
        kube::error::Error::Api(ae) if ae.code == 404 => OperatorError::SecretNotFound(format!(
            "Kubernetes Secret '{}' not found in namespace '{}'",
            ks_name, namespace
        )),
        other => OperatorError::KubeError(other),
    }
}

/// Décompose un Secret K8s déjà lu en `KsSourceState` ou en erreur métier.
/// Fonction pure — testable unitairement sans mock kube.
///
/// - opt-in label absent / != "true" → `SecretOptInMissing` permanent
/// - sinon → `KsSourceState` avec resource_version, key_present, payload extraits
fn parse_k8s_secret_source(
    ks: &Secret,
    ks_name: &str,
    key: &str,
    cr_namespace: &str,
    cr_name: &str,
) -> Result<KsSourceState> {
    if !is_opt_in_granted(ks.metadata.labels.as_ref()) {
        return Err(OperatorError::SecretOptInMissing(format!(
            "Kubernetes Secret '{}' must carry label '{}: \"true\"'",
            ks_name, OPT_IN_LABEL
        )));
    }
    if !is_cr_allowed_for_secret(ks.metadata.annotations.as_ref(), cr_namespace, cr_name) {
        return Err(OperatorError::SecretOptInMissing(format!(
            "Kubernetes Secret '{}' must carry annotation '{}: \"{}/{}\"' to be read by this ScalewaySecret",
            ks_name, ALLOWED_CR_ANNOTATION, cr_namespace, cr_name
        )));
    }

    let resource_version = ks.metadata.resource_version.clone();
    let payload = ks
        .data
        .as_ref()
        .and_then(|d| d.get(key))
        .map(|b| b.0.clone());
    let key_present = payload.is_some();

    Ok(KsSourceState {
        resource_version,
        key_present,
        payload,
    })
}

/// Lit le Secret K8s source en UN seul appel API, vérifie le label d'opt-in,
/// et retourne l'état nécessaire à la décision + le payload pour les branches actives.
///
/// Combine `map_kube_get_error` et `parse_k8s_secret_source` (tous deux testés unitairement).
async fn read_k8s_secret_source(
    ctx: &Arc<Context>,
    namespace: &str,
    ks_ref: &crate::resources::KubernetesSecretRef,
    cr_namespace: &str,
    cr_name: &str,
) -> Result<KsSourceState> {
    let ks_api: Api<Secret> = Api::namespaced(ctx.client.clone(), namespace);
    let ks = ks_api
        .get(&ks_ref.name)
        .await
        .map_err(|e| map_kube_get_error(e, &ks_ref.name, namespace))?;
    parse_k8s_secret_source(&ks, &ks_ref.name, &ks_ref.key, cr_namespace, cr_name)
}

/// Construit un status anonymisé pour les erreurs liées à la source Secret K8s.
/// Préserve les champs non liés (scaleway_id, current_version, last_synced_resource_version)
/// et écrase `error_message`/`sync_state`. Fonction pure pour test unitaire.
fn build_anonymized_source_error_status(
    current_status: &ScalewaySecretStatus,
) -> ScalewaySecretStatus {
    let mut status = current_status.clone();
    status.error_message = Some(STATUS_ERROR_GENERIC.to_string());
    status.sync_state = "Error".to_string();
    status
}

/// Met à jour le status avec un message générique (anonymise les noms de Secret/clé)
/// pour éviter la fuite par oracle d'existence depuis status.error_message.
/// Le détail reste dans les tracing logs.
async fn record_source_error_in_status(
    secret_cr: &Arc<ScalewaySecret>,
    api: &Api<ScalewaySecret>,
    current_status: &ScalewaySecretStatus,
    error: &OperatorError,
) {
    tracing::warn!(
        name = %secret_cr.name_any(),
        namespace = %secret_cr.namespace().unwrap_or_default(),
        error = %error,
        "ScalewaySecret source read failed"
    );
    let status = build_anonymized_source_error_status(current_status);
    let _ = update_secret_status(secret_cr, api, status).await;
}

/// Snapshot immuable pour la décision de réconciliation du ScalewaySecret.
struct SecretReconcileInput {
    deletion_requested: bool,
    circuit_open: bool,
    finalizer_present: bool,
    /// La source (kubernetes_secret) est configurée dans le spec.
    source_configured: bool,
    /// ID Scaleway du secret, depuis le status.
    scaleway_id: Option<String>,
    /// resourceVersion du Secret K8s source à la dernière synchronisation,
    /// depuis le status.
    last_synced_resource_version: Option<String>,
    /// resourceVersion courant du Secret K8s source.
    /// None = K8s Secret introuvable (erreur transitoire).
    current_resource_version: Option<String>,
    /// La clé `spec.source.kubernetes_secret.key` existe dans `.data` du Secret K8s.
    /// Pertinent uniquement quand current_resource_version est Some.
    current_key_present: bool,
}

#[derive(Debug)]
enum SecretReconcileDecision {
    SkipCircuitOpen,
    AddFinalizer,
    /// Pas de source valide dans le spec — erreur permanente.
    ErrorSourceNotConfigured,
    /// Le K8s Secret source est absent — erreur transitoire.
    ErrorKsSecretNotFound,
    /// Le K8s Secret existe mais la clé référencée est absente de `.data` — erreur transitoire.
    ErrorKsKeyNotFound,
    /// Aucun secret Scaleway connu — créer et pousser la première version.
    CreateAndSyncSecret,
    /// Secret Scaleway connu, valeur changée — pousser une nouvelle version.
    PushNewVersion {
        scaleway_id: String,
    },
    /// Valeur inchangée — requeue périodique.
    AlreadySynced,
    /// Suppression demandée avec un secret Scaleway connu.
    DeleteSecret,
    /// Suppression sans secret Scaleway connu — retirer le finalizer.
    RemoveFinalizer,
}

fn decide_next_action_secret(input: &SecretReconcileInput) -> SecretReconcileDecision {
    // 1. Suppression prioritaire
    if input.deletion_requested {
        return match &input.scaleway_id {
            Some(_) => SecretReconcileDecision::DeleteSecret,
            None => SecretReconcileDecision::RemoveFinalizer,
        };
    }
    // 2. Circuit breaker
    if input.circuit_open {
        return SecretReconcileDecision::SkipCircuitOpen;
    }
    // 3. Finalizer
    if !input.finalizer_present {
        return SecretReconcileDecision::AddFinalizer;
    }
    // 4. Source non configurée (permanent)
    if !input.source_configured {
        return SecretReconcileDecision::ErrorSourceNotConfigured;
    }
    // 5. K8s Secret introuvable (transitoire)
    if input.current_resource_version.is_none() {
        return SecretReconcileDecision::ErrorKsSecretNotFound;
    }
    // 5bis. K8s Secret présent mais clé manquante dans .data (transitoire — distinct du cas Secret absent)
    if !input.current_key_present {
        return SecretReconcileDecision::ErrorKsKeyNotFound;
    }
    // 6. Pas de secret Scaleway connu — créer
    if input.scaleway_id.is_none() {
        return SecretReconcileDecision::CreateAndSyncSecret;
    }
    // 7. Source modifiée depuis la dernière synchro — nouvelle version
    if input.current_resource_version != input.last_synced_resource_version {
        return SecretReconcileDecision::PushNewVersion {
            scaleway_id: input.scaleway_id.clone().unwrap(),
        };
    }
    // 8. Rien à faire
    SecretReconcileDecision::AlreadySynced
}

pub async fn reconcile_scaleway_secret(
    secret_cr: Arc<ScalewaySecret>,
    ctx: Arc<Context>,
) -> std::result::Result<Action, OperatorError> {
    let key = format!(
        "scalewayssecret/{}/{}",
        secret_cr.namespace().unwrap_or_default(),
        secret_cr.name_any()
    );
    let result = reconcile_scaleway_secret_inner(secret_cr, ctx.clone()).await;
    if result.is_ok() {
        ctx.reset_retry_count(&key);
    }
    result
}

async fn reconcile_scaleway_secret_inner(
    secret_cr: Arc<ScalewaySecret>,
    ctx: Arc<Context>,
) -> std::result::Result<Action, OperatorError> {
    let namespace = secret_cr.namespace().unwrap_or_default();
    let api: Api<ScalewaySecret> = Api::namespaced(ctx.client.clone(), &namespace);

    tracing::info!(
        name = %secret_cr.name_any(),
        namespace = %namespace,
        "Reconciling ScalewaySecret"
    );

    let deletion_requested = secret_cr.metadata.deletion_timestamp.is_some();
    let circuit_open = ctx.is_circuit_open();
    let finalizer_present = secret_cr
        .metadata
        .finalizers
        .as_ref()
        .unwrap_or(&vec![])
        .contains(&SECRET_FINALIZER.to_string());

    let source_configured = secret_cr.spec.source.kubernetes_secret.is_some();

    let current_status = secret_cr.status.clone().unwrap_or_default();

    // Lecture UNIQUE du Secret K8s source — la TOCTOU sur le label d'opt-in est éliminée
    // car la valeur extraite (payload) provient du MÊME `get` qui a vérifié l'opt-in.
    // Les branches CreateAndSyncSecret et PushNewVersion réutilisent `ks_payload`
    // au lieu de re-lire le Secret (et de risquer une fenêtre de race).
    let (current_resource_version, current_key_present, mut ks_payload) =
        if !deletion_requested && source_configured {
            let ks_ref = secret_cr.spec.source.kubernetes_secret.as_ref().unwrap();
            let cr_name = secret_cr.name_any();
            match read_k8s_secret_source(&ctx, &namespace, ks_ref, &namespace, &cr_name).await {
                Ok(state) => (state.resource_version, state.key_present, state.payload),
                Err(OperatorError::SecretNotFound(_)) => {
                    // Cas "Secret K8s absent" — traité par le decide layer via ErrorKsSecretNotFound.
                    (None, false, None)
                }
                Err(e) => {
                    record_source_error_in_status(&secret_cr, &api, &current_status, &e).await;
                    return Err(e);
                }
            }
        } else {
            (None, false, None)
        };

    let input = SecretReconcileInput {
        deletion_requested,
        circuit_open,
        finalizer_present,
        source_configured,
        scaleway_id: current_status.scaleway_id.clone(),
        last_synced_resource_version: current_status.last_synced_resource_version.clone(),
        current_resource_version: current_resource_version.clone(),
        current_key_present,
    };

    let decision = decide_next_action_secret(&input);

    match decision {
        SecretReconcileDecision::SkipCircuitOpen => {
            tracing::warn!(
                name = %secret_cr.name_any(),
                "Scaleway API circuit breaker is open — skipping secret reconciliation"
            );
            Err(OperatorError::CircuitBreakerOpen)
        }

        SecretReconcileDecision::AddFinalizer => {
            add_secret_finalizer(&secret_cr, &api).await?;
            Ok(Action::requeue(Duration::from_secs(5)))
        }

        SecretReconcileDecision::ErrorSourceNotConfigured => {
            let mut measurer = ReconcileMeasurer::new(&ctx.metrics, &ctx.last_reconcile_at);
            let e = OperatorError::SecretSourceNotConfigured(
                "spec.source must have kubernetes_secret configured".to_string(),
            );
            let mut status = current_status;
            status.error_message = Some(e.for_status());
            status.sync_state = "Error".to_string();
            let _ = update_secret_status(&secret_cr, &api, status).await;
            measurer.set_outcome(ReconcileOutcome::Error);
            Err(e)
        }

        SecretReconcileDecision::ErrorKsSecretNotFound => {
            let ks_name = secret_cr
                .spec
                .source
                .kubernetes_secret
                .as_ref()
                .map(|r| r.name.as_str())
                .unwrap_or("<unknown>");
            // Détail dans les logs uniquement ; status reste générique pour éviter
            // l'oracle de présence des Secrets via status.error_message.
            let e = OperatorError::SecretNotFound(format!(
                "Kubernetes Secret '{}' not found in namespace '{}'",
                ks_name, namespace
            ));
            record_source_error_in_status(&secret_cr, &api, &current_status, &e).await;
            Err(e)
        }

        SecretReconcileDecision::ErrorKsKeyNotFound => {
            let (ks_name, ks_key) = secret_cr
                .spec
                .source
                .kubernetes_secret
                .as_ref()
                .map(|r| (r.name.as_str(), r.key.as_str()))
                .unwrap_or(("<unknown>", "<unknown>"));
            // SecretKeyNotFound est permanent : retry exponentiel infini sur une clé
            // absente est un DoS auto-infligé puisque seule une édition du CR ou du
            // Secret la fait apparaître. Détail dans logs, status générique.
            let e = OperatorError::SecretKeyNotFound(format!(
                "Key '{}' not found in Kubernetes Secret '{}' in namespace '{}'",
                ks_key, ks_name, namespace
            ));
            record_source_error_in_status(&secret_cr, &api, &current_status, &e).await;
            Err(e)
        }

        SecretReconcileDecision::CreateAndSyncSecret => {
            let mut measurer = ReconcileMeasurer::new(&ctx.metrics, &ctx.last_reconcile_at);

            let project_id =
                match get_project_id_from_namespace_resource(secret_cr.as_ref(), &ctx).await {
                    Ok(pid) => pid,
                    Err(e) => {
                        let mut status = current_status;
                        status.error_message = Some(e.for_status());
                        status.sync_state = "Error".to_string();
                        let _ = update_secret_status(&secret_cr, &api, status).await;
                        measurer.set_outcome(ReconcileOutcome::Error);
                        return Err(e);
                    }
                };

            let ns_client = match get_namespace_client(&ctx, &namespace).await {
                Ok(c) => c,
                Err(e) => {
                    let mut status = current_status;
                    status.error_message = Some(e.for_status());
                    status.sync_state = "Error".to_string();
                    let _ = update_secret_status(&secret_cr, &api, status).await;
                    measurer.set_outcome(ReconcileOutcome::Error);
                    return Err(e);
                }
            };

            // Adoption : chercher un secret Scaleway existant avec les tags opérateur
            let cr_name = secret_cr.name_any();
            let scaleway_id = match call_scaleway(&ctx, || {
                ns_client.find_scaleway_secret_by_tags(
                    &secret_cr.spec.region,
                    &project_id,
                    &namespace,
                    &cr_name,
                )
            })
            .await?
            {
                Some(existing_id) => {
                    tracing::warn!(
                        name = %cr_name,
                        scaleway_id = %existing_id,
                        "Adopted existing Scaleway secret"
                    );
                    existing_id
                }
                None => call_scaleway(&ctx, || {
                    ns_client.create_scaleway_secret(
                        &secret_cr.spec.region,
                        &secret_cr.spec.name,
                        &project_id,
                        secret_cr.spec.description.as_deref(),
                        &secret_cr.spec.tags,
                        &namespace,
                        &cr_name,
                    )
                })
                .await
                .inspect_err(|_| {
                    measurer.set_outcome(ReconcileOutcome::Error);
                })?,
            };

            // Reserve-intent (issue #117) : ancrer le `scaleway_id` dans le status K8s
            // AVANT tout `create_secret_version`. Sans ce patch préliminaire, si
            // `create_secret_version` réussit puis le PATCH status final échoue
            // (conflit 409, webhook tiers, kube-apiserver transient), le reconcile
            // suivant ré-entre dans `CreateAndSyncSecret` (scaleway_id=None dans le
            // status), `find_scaleway_secret_by_tags` adopte le secret existant, et
            // un 2e `create_secret_version` crée une version active en double.
            //
            // Avec ce patch préliminaire :
            //  - Si l'update échoue : pas encore de side-effect Scaleway sur les
            //    versions → aucune dérive (le `?` propage l'Err avant
            //    `create_secret_version`).
            //  - Si `create_secret_version` réussit puis l'update final échoue : le
            //    reconcile suivant aura `scaleway_id=Some(...)` et
            //    `last_synced_resource_version=None`, donc `decide_next_action_secret`
            //    retourne `PushNewVersion` (pas `CreateAndSyncSecret`). La branche
            //    `PushNewVersion` applique le pattern #114 (update_status d'abord),
            //    et le secret Scaleway ne sera pas re-créé.
            //
            // Trade-off : un cycle de réconciliation supplémentaire en cas d'échec
            // entre étape 3 et étape 6, contre l'élimination du risque de
            // duplication du secret Scaleway côté adoption.
            let mut prelim_status = current_status.clone();
            prelim_status.scaleway_id = Some(scaleway_id.clone());
            prelim_status.sync_state = "Syncing".to_string();
            prelim_status.error_message = None;
            prelim_status.current_version = None;
            prelim_status.last_synced_resource_version = None;
            update_secret_status(&secret_cr, &api, prelim_status)
                .await
                .inspect_err(|_| {
                    measurer.set_outcome(ReconcileOutcome::Error);
                })?;

            // Payload et resource_version proviennent de la lecture UNIQUE en amont —
            // évite la TOCTOU entre la vérification opt-in et la lecture de la valeur.
            let payload = ks_payload.take().expect(
                "decide_next_action_secret guarantees key_present and current_resource_version",
            );

            // Si create_secret_version échoue après le patch préliminaire "Syncing",
            // il faut transitionner le status vers "Error" sinon le CR reste en
            // "Syncing" indéfiniment (le prochain reconcile entrerait par PushNewVersion
            // et n'effacerait pas cet état tant que le Secret K8s source reste lisible).
            // Revue Opus correctness — finding observabilité de l'issue #117.
            let revision = match call_scaleway(&ctx, || {
                ns_client.create_secret_version(&secret_cr.spec.region, &scaleway_id, &payload)
            })
            .await
            {
                Ok(r) => r,
                Err(e) => {
                    let mut err_status = current_status.clone();
                    err_status.scaleway_id = Some(scaleway_id.clone());
                    err_status.sync_state = "Error".to_string();
                    err_status.error_message = Some(e.for_status());
                    let _ = update_secret_status(&secret_cr, &api, err_status).await;
                    measurer.set_outcome(ReconcileOutcome::Error);
                    return Err(e);
                }
            };

            let mut status = current_status;
            status.scaleway_id = Some(scaleway_id);
            status.current_version = Some(revision);
            status.last_synced_resource_version = current_resource_version.clone();
            status.sync_state = "Synced".to_string();
            status.error_message = None;
            update_secret_status(&secret_cr, &api, status).await?;

            measurer.set_outcome(ReconcileOutcome::Created);
            Ok(Action::requeue(Duration::from_secs(30)))
        }

        SecretReconcileDecision::PushNewVersion { scaleway_id } => {
            let mut measurer = ReconcileMeasurer::new(&ctx.metrics, &ctx.last_reconcile_at);

            let ns_client = match get_namespace_client(&ctx, &namespace).await {
                Ok(c) => c,
                Err(e) => {
                    let mut status = current_status;
                    status.error_message = Some(e.for_status());
                    status.sync_state = "Error".to_string();
                    let _ = update_secret_status(&secret_cr, &api, status).await;
                    measurer.set_outcome(ReconcileOutcome::Error);
                    return Err(e);
                }
            };

            // Payload et resource_version proviennent de la lecture UNIQUE en amont.
            let payload = ks_payload.take().expect(
                "decide_next_action_secret guarantees key_present and current_resource_version",
            );

            let old_revision = current_status.current_version;

            let new_revision = call_scaleway(&ctx, || {
                ns_client.create_secret_version(&secret_cr.spec.region, &scaleway_id, &payload)
            })
            .await
            .inspect_err(|_| {
                measurer.set_outcome(ReconcileOutcome::Error);
            })?;

            // Tracker la nouvelle révision AVANT de tenter le disable de l'ancienne.
            // Invariant (issue #114) : si le status est mis à jour en premier avec
            // `last_synced_resource_version = current_resource_version`, alors un
            // échec ultérieur sur disable ne déclenche pas un re-PushNewVersion au
            // prochain reconcile (la décision sera AlreadySynced). Sans cet ordre,
            // chaque échec transitoire sur disable créerait une version active
            // supplémentaire sur Scaleway → dérive.
            //
            // Limite explicite (revue Opus correctness) : la garantie est conditionnée
            // au succès de `update_secret_status` (le `?` ligne suivante propage l'Err).
            // Si la mise à jour du status K8s échoue durablement (5xx persistant,
            // conflit 409, kube-apiserver down) ALORS que `create_secret_version` a
            // réussi, le reconcile suivant re-décidera `PushNewVersion` et créera une
            // 2e version active sur Scaleway. Probabilité faible (PATCH idempotent +
            // retry kube-rs) mais non nulle. La fix #114 réduit donc la fenêtre de
            // dérive sans l'éliminer totalement.
            let mut status = current_status;
            status.current_version = Some(new_revision);
            status.last_synced_resource_version = current_resource_version.clone();
            status.sync_state = "Synced".to_string();
            status.error_message = None;
            update_secret_status(&secret_cr, &api, status).await?;

            // Désactiver l'ancienne version en best-effort (idempotent si déjà désactivée).
            // Trade-off accepté (issue #114) : si le disable échoue durablement, l'ancienne
            // version reste `enabled` sur Scaleway jusqu'à intervention manuelle ou prochaine
            // rotation. La nouvelle version est correctement référencée et active.
            // On préfère cette dérive bornée à la création répétée de nouvelles versions
            // à chaque reconcile en cas d'échec transitoire du disable.
            if let Some(old_rev) = old_revision {
                if let Err(e) = call_scaleway(&ctx, || {
                    ns_client.disable_secret_version(&secret_cr.spec.region, &scaleway_id, old_rev)
                })
                .await
                {
                    tracing::warn!(
                        name = %secret_cr.name_any(),
                        revision = old_rev,
                        error = %e,
                        "Failed to disable old secret version — best-effort, will not fail reconcile (see issue #114)"
                    );
                }
            }

            measurer.set_outcome(ReconcileOutcome::Synced);
            Ok(Action::requeue(Duration::from_secs(30)))
        }

        SecretReconcileDecision::AlreadySynced => {
            let mut measurer = ReconcileMeasurer::new(&ctx.metrics, &ctx.last_reconcile_at);
            measurer.set_outcome(ReconcileOutcome::Synced);
            Ok(Action::requeue(Duration::from_secs(30)))
        }

        SecretReconcileDecision::DeleteSecret => {
            let mut measurer = ReconcileMeasurer::new(&ctx.metrics, &ctx.last_reconcile_at);

            if let Some(scaleway_id) = &current_status.scaleway_id {
                match get_namespace_client(&ctx, &namespace).await {
                    Ok(ns_client) => {
                        if let Err(e) = call_scaleway(&ctx, || {
                            ns_client.delete_scaleway_secret(&secret_cr.spec.region, scaleway_id)
                        })
                        .await
                        {
                            tracing::error!(
                                name = %secret_cr.name_any(),
                                error = %e,
                                "Failed to delete Scaleway secret"
                            );
                            measurer.set_outcome(ReconcileOutcome::Error);
                            return Err(e);
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            name = %secret_cr.name_any(),
                            scaleway_id = %scaleway_id,
                            error = %e,
                            "IAM Secret missing during deletion — skipping Scaleway API call"
                        );
                    }
                }
            }

            // Retirer le finalizer
            let finalizers = secret_cr.metadata.finalizers.clone().unwrap_or_default();
            let new_finalizers: Vec<String> = finalizers
                .into_iter()
                .filter(|f| f != SECRET_FINALIZER)
                .collect();

            let patch = serde_json::json!({ "metadata": { "finalizers": new_finalizers } });
            api.patch(
                &secret_cr.name_any(),
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

        SecretReconcileDecision::RemoveFinalizer => {
            let mut measurer = ReconcileMeasurer::new(&ctx.metrics, &ctx.last_reconcile_at);
            let finalizers = secret_cr.metadata.finalizers.clone().unwrap_or_default();
            let new_finalizers: Vec<String> = finalizers
                .into_iter()
                .filter(|f| f != SECRET_FINALIZER)
                .collect();

            let patch = serde_json::json!({ "metadata": { "finalizers": new_finalizers } });
            api.patch(
                &secret_cr.name_any(),
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
    }
}

async fn add_secret_finalizer(secret_cr: &ScalewaySecret, api: &Api<ScalewaySecret>) -> Result<()> {
    let mut finalizers = secret_cr.metadata.finalizers.clone().unwrap_or_default();
    finalizers.push(SECRET_FINALIZER.to_string());
    let patch = serde_json::json!({ "metadata": { "finalizers": finalizers } });
    api.patch(
        &secret_cr.name_any(),
        &PatchParams::default(),
        &Patch::Merge(patch),
    )
    .await?;
    Ok(())
}

async fn update_secret_status(
    secret_cr: &ScalewaySecret,
    api: &Api<ScalewaySecret>,
    status: ScalewaySecretStatus,
) -> Result<()> {
    let patch = serde_json::json!({ "status": status });
    api.patch_status(
        &secret_cr.name_any(),
        &PatchParams::default(),
        &Patch::Merge(patch),
    )
    .await?;
    Ok(())
}

fn error_policy_inner(key: String, error: &OperatorError, ctx: &Arc<Context>) -> Action {
    if error.is_permanent_error() {
        tracing::warn!(error = %error, "Permanent configuration error — waiting for spec change");
        Action::await_change()
    } else {
        let attempts = ctx.increment_retry_count(&key);
        // Backoff exponentiel : 30s, 60s, 120s, 240s, 300s (max)
        let delay_secs = (30u64 * (1u64 << (attempts - 1).min(9))).min(300);
        if matches!(error, OperatorError::CircuitBreakerOpen) {
            tracing::warn!(
                attempts = attempts,
                retry_in_secs = delay_secs,
                "Scaleway API circuit breaker open — backing off"
            );
        } else {
            tracing::error!(error = %error, attempts = attempts, retry_in_secs = delay_secs, "Transient reconciliation error");
        }
        Action::requeue(Duration::from_secs(delay_secs))
    }
}

pub fn error_policy<R: kube::ResourceExt>(
    kind: &'static str,
    resource: Arc<R>,
    error: &OperatorError,
    ctx: Arc<Context>,
) -> Action {
    ctx.metrics.record_error(error);
    let key = format!(
        "{}/{}/{}",
        kind,
        resource.namespace().unwrap_or_default(),
        resource.name_any()
    );
    error_policy_inner(key, error, &ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── decide_next_action unit tests ───────────────────────────────────────

    fn base_input() -> ReconcileInput {
        ReconcileInput {
            deletion_requested: false,
            circuit_open: false,
            finalizer_present: true,
            scaleway_role: "Editor".to_string(),
            project_id: "11111111-1111-1111-1111-111111111111".to_string(),
            scaleway_id: None,
            // status_project_id défini → CreateInstance (pas VerifyProjectAccess)
            status_project_id: Some("11111111-1111-1111-1111-111111111111".to_string()),
        }
    }

    #[test]
    fn test_decide_circuit_open_returns_skip() {
        let input = ReconcileInput {
            circuit_open: true,
            ..base_input()
        };
        assert!(matches!(
            decide_next_action(&input),
            ReconcileDecision::SkipCircuitOpen
        ));
    }

    #[test]
    fn test_decide_finalizer_absent_returns_add_finalizer() {
        let input = ReconcileInput {
            finalizer_present: false,
            ..base_input()
        };
        assert!(matches!(
            decide_next_action(&input),
            ReconcileDecision::AddFinalizer
        ));
    }

    #[test]
    fn test_decide_readonly_role_no_scaleway_id_returns_block() {
        let input = ReconcileInput {
            scaleway_role: "Viewer".to_string(),
            ..base_input()
        };
        assert!(matches!(
            decide_next_action(&input),
            ReconcileDecision::BlockReadOnlyRole
        ));
    }

    #[test]
    fn test_decide_no_scaleway_id_write_role_returns_create() {
        let input = base_input();
        let decision = decide_next_action(&input);
        let ReconcileDecision::CreateInstance { project_id } = decision else {
            panic!("expected CreateInstance, got {decision:?}");
        };
        // Verrouille la propagation du project_id depuis l'input — verrouille contre
        // un copier-coller `input.scaleway_id` au lieu de `input.project_id`.
        assert_eq!(project_id, input.project_id);
    }

    #[test]
    fn test_decide_scaleway_id_present_returns_sync() {
        let input = ReconcileInput {
            scaleway_id: Some("srv-abc123".to_string()),
            ..base_input()
        };
        let decision = decide_next_action(&input);
        let ReconcileDecision::SyncInstance {
            scaleway_id,
            project_id,
        } = decision
        else {
            panic!("expected SyncInstance, got {decision:?}");
        };
        assert_eq!(scaleway_id, "srv-abc123");
        assert_eq!(project_id, input.project_id);
    }

    #[test]
    fn test_decide_no_status_project_id_returns_verify_with_project_id() {
        // Branche VerifyProjectAccess : doit propager input.project_id (jamais
        // input.scaleway_id ni autre champ — guard contre copier-coller).
        let input = ReconcileInput {
            status_project_id: None,
            ..base_input()
        };
        let decision = decide_next_action(&input);
        let ReconcileDecision::VerifyProjectAccess { project_id } = decision else {
            panic!("expected VerifyProjectAccess, got {decision:?}");
        };
        assert_eq!(project_id, input.project_id);
    }

    #[test]
    fn test_decide_deletion_with_scaleway_id_returns_delete() {
        let input = ReconcileInput {
            deletion_requested: true,
            scaleway_id: Some("srv-abc123".to_string()),
            ..base_input()
        };
        assert!(matches!(
            decide_next_action(&input),
            ReconcileDecision::DeleteInstance
        ));
    }

    #[test]
    fn test_decide_deletion_without_scaleway_id_returns_remove_finalizer() {
        let input = ReconcileInput {
            deletion_requested: true,
            scaleway_id: None,
            ..base_input()
        };
        assert!(matches!(
            decide_next_action(&input),
            ReconcileDecision::RemoveFinalizer
        ));
    }

    #[test]
    fn test_decide_deletion_takes_priority_over_circuit_open() {
        // Invariant critique : la suppression doit fonctionner même quand l'API
        // Scaleway est down. Sinon un user ne peut pas retirer un finalizer pendant
        // une panne et son CR reste bloqué indéfiniment.
        let input = ReconcileInput {
            deletion_requested: true,
            circuit_open: true,
            scaleway_id: Some("srv-abc".to_string()),
            ..base_input()
        };
        assert!(matches!(
            decide_next_action(&input),
            ReconcileDecision::DeleteInstance
        ));
    }

    #[test]
    fn test_decide_finalizer_added_before_role_check() {
        // Invariant : le finalizer doit être ajouté AVANT toute vérification de rôle
        // ou tout side effect Scaleway. Sinon un Secret IAM Viewer avec finalizer
        // absent serait bloqué sur BlockReadOnlyRole alors qu'il devrait d'abord
        // recevoir le finalizer.
        let input = ReconcileInput {
            finalizer_present: false,
            scaleway_role: "Viewer".to_string(),
            ..base_input()
        };
        assert!(matches!(
            decide_next_action(&input),
            ReconcileDecision::AddFinalizer
        ));
    }

    #[test]
    fn test_decide_circuit_open_skips_even_when_finalizer_absent() {
        // Documente le comportement actuel : circuit_open prend la priorité sur
        // l'ajout du finalizer. Argument design : éviter toute interaction avec
        // l'API en cas de cascade d'erreur. Cohérent dans les 3 reconcilers
        // (Instance / LB / Secret). À reconsidérer si une PR future veut
        // dissocier les actions purement K8s du circuit breaker.
        let input = ReconcileInput {
            finalizer_present: false,
            circuit_open: true,
            ..base_input()
        };
        assert!(matches!(
            decide_next_action(&input),
            ReconcileDecision::SkipCircuitOpen
        ));
    }

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

    // ── decide_next_action_lb unit tests ───────────────────────────────────────

    fn base_lb_input() -> LbReconcileInput {
        LbReconcileInput {
            deletion_requested: false,
            circuit_open: false,
            finalizer_present: true,
            scaleway_role: "Editor".to_string(),
            project_id: "11111111-1111-1111-1111-111111111111".to_string(),
            scaleway_id: None,
            status_project_id: Some("11111111-1111-1111-1111-111111111111".to_string()),
        }
    }

    #[test]
    fn test_lb_decide_default_base_returns_create() {
        let input = base_lb_input();
        let decision = decide_next_action_lb(&input);
        let LbReconcileDecision::CreateLoadBalancer { project_id } = decision else {
            panic!("expected CreateLoadBalancer, got {decision:?}");
        };
        assert_eq!(project_id, input.project_id);
    }

    #[test]
    fn test_lb_decide_finalizer_absent_returns_add_finalizer() {
        let input = LbReconcileInput {
            finalizer_present: false,
            ..base_lb_input()
        };
        assert!(matches!(
            decide_next_action_lb(&input),
            LbReconcileDecision::AddLbFinalizer
        ));
    }

    #[test]
    fn test_lb_decide_circuit_open_returns_skip() {
        let input = LbReconcileInput {
            circuit_open: true,
            ..base_lb_input()
        };
        assert!(matches!(
            decide_next_action_lb(&input),
            LbReconcileDecision::SkipCircuitOpen
        ));
    }

    #[test]
    fn test_lb_decide_scaleway_id_present_returns_sync() {
        let input = LbReconcileInput {
            scaleway_id: Some("lb-abc".to_string()),
            ..base_lb_input()
        };
        let decision = decide_next_action_lb(&input);
        let LbReconcileDecision::SyncLoadBalancer {
            scaleway_id,
            project_id,
        } = decision
        else {
            panic!("expected SyncLoadBalancer, got {decision:?}");
        };
        assert_eq!(scaleway_id, "lb-abc");
        assert_eq!(project_id, input.project_id);
    }

    #[test]
    fn test_lb_decide_deletion_with_scaleway_id_returns_delete() {
        let input = LbReconcileInput {
            deletion_requested: true,
            scaleway_id: Some("lb-abc".to_string()),
            ..base_lb_input()
        };
        assert!(matches!(
            decide_next_action_lb(&input),
            LbReconcileDecision::DeleteLoadBalancer
        ));
    }

    #[test]
    fn test_lb_decide_deletion_without_scaleway_id_returns_remove_finalizer() {
        let input = LbReconcileInput {
            deletion_requested: true,
            scaleway_id: None,
            ..base_lb_input()
        };
        assert!(matches!(
            decide_next_action_lb(&input),
            LbReconcileDecision::RemoveLbFinalizer
        ));
    }

    #[test]
    fn test_lb_decide_deletion_takes_priority_over_circuit_open() {
        // Cf. test_decide_deletion_takes_priority_over_circuit_open (Instance) —
        // la suppression doit fonctionner même quand l'API Scaleway est down.
        let input = LbReconcileInput {
            deletion_requested: true,
            circuit_open: true,
            scaleway_id: Some("lb-abc".to_string()),
            ..base_lb_input()
        };
        assert!(matches!(
            decide_next_action_lb(&input),
            LbReconcileDecision::DeleteLoadBalancer
        ));
    }

    #[test]
    fn test_lb_decide_finalizer_added_before_role_check() {
        // L'ajout du finalizer précède toute vérification Scaleway.
        let input = LbReconcileInput {
            finalizer_present: false,
            scaleway_role: "Viewer".to_string(),
            ..base_lb_input()
        };
        assert!(matches!(
            decide_next_action_lb(&input),
            LbReconcileDecision::AddLbFinalizer
        ));
    }

    #[test]
    fn test_lb_decide_circuit_open_skips_even_when_finalizer_absent() {
        // Cf. test_decide_circuit_open_skips_even_when_finalizer_absent (Instance).
        let input = LbReconcileInput {
            finalizer_present: false,
            circuit_open: true,
            ..base_lb_input()
        };
        assert!(matches!(
            decide_next_action_lb(&input),
            LbReconcileDecision::SkipCircuitOpen
        ));
    }

    #[test]
    fn test_lb_decide_viewer_role_returns_block() {
        let input = LbReconcileInput {
            scaleway_role: "Viewer".to_string(),
            ..base_lb_input()
        };
        assert!(matches!(
            decide_next_action_lb(&input),
            LbReconcileDecision::BlockReadOnlyRole
        ));
    }

    #[test]
    fn test_lb_decide_no_status_project_id_returns_verify() {
        let input = LbReconcileInput {
            status_project_id: None,
            ..base_lb_input()
        };
        let decision = decide_next_action_lb(&input);
        let LbReconcileDecision::VerifyProjectAccessLb { project_id } = decision else {
            panic!("expected VerifyProjectAccessLb, got {decision:?}");
        };
        assert_eq!(project_id, input.project_id);
    }

    // --- retry_counts key format ---

    #[test]
    fn test_retry_key_instance_has_kind_prefix() {
        // Vérifie que le format "instance/{ns}/{name}" n'entre pas en collision
        // avec "loadbalancer/{ns}/{name}" pour des ressources homonymes.
        let instance_key = format!("instance/{}/{}", "production", "web");
        let lb_key = format!("loadbalancer/{}/{}", "production", "web");
        assert_ne!(
            instance_key, lb_key,
            "instance and LB keys must not collide"
        );
        assert!(instance_key.starts_with("instance/"));
        assert!(lb_key.starts_with("loadbalancer/"));
    }

    // ── ReconcileMeasurer unit tests ─────────────────────────────────────────

    fn fresh_metrics() -> OperatorMetrics {
        OperatorMetrics::new(&prometheus::Registry::new()).unwrap()
    }

    fn histogram_sample_count(metrics: &OperatorMetrics, outcome_label: &str) -> u64 {
        metrics
            .reconcile_duration_seconds
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
        assert_eq!(
            histogram_sample_count(&metrics, "Error"),
            1,
            "Error histogram should have 1 observation after drop-without-outcome"
        );
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
            retry_counts: std::sync::Mutex::new(std::collections::HashMap::new()),
            circuit_breaker: std::sync::Mutex::new(crate::context::CircuitBreakerState::Closed {
                failure_count: 0,
            }),
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
        error_policy("instance", dummy_instance(), &err, ctx.clone());
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
        error_policy("instance", dummy_instance(), &err, ctx.clone());
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
        error_policy("instance", dummy_instance(), &err, ctx.clone());
        error_policy("instance", dummy_instance(), &err, ctx.clone());
        let value = ctx
            .metrics
            .reconcile_errors_total
            .with_label_values(&["ConfigError"])
            .get();
        assert_eq!(value, 2);
    }

    #[tokio::test]
    async fn test_error_policy_permanent_error_returns_await_change() {
        let ctx = make_test_context();
        for err in [
            OperatorError::InvalidZone("bad".to_string()),
            OperatorError::InvalidInstanceType("bad".to_string()),
            OperatorError::InvalidLbType("bad".to_string()),
            OperatorError::ConfigError("bad".to_string()),
            OperatorError::ProjectAccessDenied("bad".to_string()),
        ] {
            let action = error_policy("instance", dummy_instance(), &err, ctx.clone());
            assert_eq!(
                action,
                Action::await_change(),
                "expected await_change for {err:?}"
            );
        }
    }

    #[tokio::test]
    async fn test_error_policy_transient_error_returns_requeue_with_backoff() {
        let ctx = make_test_context();
        let err = OperatorError::Unknown("transient".to_string());

        // 1re tentative → 30s
        let action = error_policy("instance", dummy_instance(), &err, ctx.clone());
        assert_eq!(action, Action::requeue(Duration::from_secs(30)));

        // 2e tentative → 60s
        let action = error_policy("instance", dummy_instance(), &err, ctx.clone());
        assert_eq!(action, Action::requeue(Duration::from_secs(60)));

        // 3e tentative → 120s
        let action = error_policy("instance", dummy_instance(), &err, ctx.clone());
        assert_eq!(action, Action::requeue(Duration::from_secs(120)));
    }

    #[tokio::test]
    async fn test_error_policy_circuit_breaker_returns_requeue() {
        let ctx = make_test_context();
        let err = OperatorError::CircuitBreakerOpen;
        let action = error_policy("instance", dummy_instance(), &err, ctx.clone());
        // CircuitBreakerOpen est transitoire : requeue (pas await_change)
        assert!(matches!(action, Action { .. }));
        assert_ne!(action, Action::await_change());
    }

    #[tokio::test]
    async fn test_error_policy_backoff_caps_at_300s() {
        let ctx = make_test_context();
        let err = OperatorError::Unknown("transient".to_string());
        // 10 tentatives atteignent le plafond de 300s
        let mut last = Action::requeue(Duration::from_secs(0));
        for _ in 0..10 {
            last = error_policy("instance", dummy_instance(), &err, ctx.clone());
        }
        assert_eq!(last, Action::requeue(Duration::from_secs(300)));
    }

    #[tokio::test]
    async fn test_error_policy_loadbalancer_kind_uses_separate_retry_counter() {
        use crate::resources::{LoadBalancer, LoadBalancerSpec};
        let ctx = make_test_context();
        let err = OperatorError::Unknown("transient".to_string());

        let lb = Arc::new(LoadBalancer {
            metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                name: Some("test".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            spec: LoadBalancerSpec {
                name: "test".to_string(),
                zone: "fr-par-1".to_string(),
                lb_type: "LB-S".to_string(),
                description: None,
                tags: vec![],
            },
            status: None,
        });

        // Instance et LB partagent le même namespace/name mais des compteurs distincts
        error_policy("instance", dummy_instance(), &err, ctx.clone());
        let lb_action = error_policy("loadbalancer", lb, &err, ctx.clone());

        // Le LB est à la 1re tentative (30s), pas à la 2e (60s)
        assert_eq!(lb_action, Action::requeue(Duration::from_secs(30)));
    }

    // ── decide_next_action_secret unit tests ───────────────────────────────────

    fn base_secret_input() -> SecretReconcileInput {
        SecretReconcileInput {
            deletion_requested: false,
            circuit_open: false,
            finalizer_present: true,
            source_configured: true,
            scaleway_id: Some("sec-abc123".to_string()),
            last_synced_resource_version: Some("12345".to_string()),
            current_resource_version: Some("12345".to_string()),
            current_key_present: true,
        }
    }

    #[test]
    fn test_secret_decide_already_synced() {
        let input = base_secret_input();
        assert!(matches!(
            decide_next_action_secret(&input),
            SecretReconcileDecision::AlreadySynced
        ));
    }

    /// Documentation d'invariant pour l'issue #114 — couche PURE uniquement.
    ///
    /// Sous l'hypothèse que `reconcile_scaleway_secret_inner` appelle bien
    /// `update_secret_status` AVANT `disable_secret_version` (l'ordre fixé par
    /// #114), le decide layer garantit que `last_synced_resource_version ==
    /// current_resource_version` mène à `AlreadySynced` — donc pas de
    /// re-PushNewVersion même si le disable a échoué.
    ///
    /// ⚠️ Ce test NE verrouille PAS l'ordre des side-effects dans la couche
    /// I/O — il documente seulement l'invariant decide qui rend la fix
    /// correcte. Un test mockito sur `reconcile_scaleway_secret_inner` avec
    /// `create_secret_version=200` + `disable_secret_version=500` est nécessaire
    /// pour vraiment détecter une régression d'ordre. Tracé dans l'issue #118
    /// (tests d'intégration ScalewaySecret).
    #[test]
    fn test_decide_after_successful_push_with_failed_disable_is_already_synced() {
        let input = SecretReconcileInput {
            last_synced_resource_version: Some("rv-after".to_string()),
            current_resource_version: Some("rv-after".to_string()),
            ..base_secret_input()
        };
        assert!(
            matches!(
                decide_next_action_secret(&input),
                SecretReconcileDecision::AlreadySynced
            ),
            "decide layer must yield AlreadySynced when rvs match (post-push state — issue #114)"
        );
    }

    /// Documentation d'invariant pour l'issue #117 — couche PURE uniquement.
    ///
    /// Le pattern reserve-intent dans `CreateAndSyncSecret` patch un status
    /// préliminaire avec `scaleway_id=Some(...)`, `current_version=None` et
    /// `last_synced_resource_version=None` AVANT le `create_secret_version`.
    /// Si la séquence est interrompue (échec entre étape 3 et étape 6), le
    /// reconcile suivant doit retomber dans `PushNewVersion` (pas
    /// `CreateAndSyncSecret`) pour éviter une re-création du secret Scaleway
    /// via adoption + duplication de version.
    ///
    /// Ce test verrouille l'invariant decide qui rend cette stratégie correcte :
    /// `scaleway_id=Some` + `last_synced_rv=None` + `current_rv=Some` produit
    /// `PushNewVersion`. La branche PushNewVersion applique elle-même le
    /// pattern #114 (update_status d'abord, disable best-effort).
    ///
    /// ⚠️ Ce test NE verrouille PAS la séquence de PATCH côté I/O — il
    /// documente seulement l'invariant decide. Test mockito tracé dans #118.
    #[test]
    fn test_decide_after_interrupted_create_and_sync_is_push_new_version() {
        let input = SecretReconcileInput {
            scaleway_id: Some("sec-after-prelim".to_string()),
            last_synced_resource_version: None,
            current_resource_version: Some("rv-1".to_string()),
            ..base_secret_input()
        };
        match decide_next_action_secret(&input) {
            SecretReconcileDecision::PushNewVersion { scaleway_id } => {
                assert_eq!(
                    scaleway_id, "sec-after-prelim",
                    "decide must forward the persisted scaleway_id so the next reconcile reuses the existing Scaleway secret (issue #117)"
                );
            }
            other => panic!(
                "expected PushNewVersion after interrupted CreateAndSync (issue #117), got {other:?}"
            ),
        }
    }

    #[test]
    fn test_secret_decide_circuit_open_skips() {
        let input = SecretReconcileInput {
            circuit_open: true,
            ..base_secret_input()
        };
        assert!(matches!(
            decide_next_action_secret(&input),
            SecretReconcileDecision::SkipCircuitOpen
        ));
    }

    #[test]
    fn test_secret_decide_finalizer_absent_adds_finalizer() {
        let input = SecretReconcileInput {
            finalizer_present: false,
            ..base_secret_input()
        };
        assert!(matches!(
            decide_next_action_secret(&input),
            SecretReconcileDecision::AddFinalizer
        ));
    }

    #[test]
    fn test_secret_decide_source_not_configured_returns_error() {
        let input = SecretReconcileInput {
            source_configured: false,
            ..base_secret_input()
        };
        assert!(matches!(
            decide_next_action_secret(&input),
            SecretReconcileDecision::ErrorSourceNotConfigured
        ));
    }

    #[test]
    fn test_secret_decide_ks_secret_not_found_returns_error() {
        let input = SecretReconcileInput {
            current_resource_version: None,
            current_key_present: false,
            ..base_secret_input()
        };
        assert!(matches!(
            decide_next_action_secret(&input),
            SecretReconcileDecision::ErrorKsSecretNotFound
        ));
    }

    // ── is_opt_in_granted (label contract) ────────────────────────────────────

    use std::collections::BTreeMap;

    #[test]
    fn test_opt_in_granted_only_when_label_value_is_exactly_true() {
        let mut labels = BTreeMap::new();
        labels.insert(
            "scaleway.mathieubodin.io/allow-operator-read".to_string(),
            "true".to_string(),
        );
        assert!(is_opt_in_granted(Some(&labels)));
    }

    #[test]
    fn test_opt_in_denied_when_no_labels() {
        assert!(!is_opt_in_granted(None));
    }

    #[test]
    fn test_opt_in_denied_when_label_absent() {
        let labels: BTreeMap<String, String> = BTreeMap::new();
        assert!(!is_opt_in_granted(Some(&labels)));
    }

    #[test]
    fn test_opt_in_denied_when_label_value_capitalized() {
        let mut labels = BTreeMap::new();
        labels.insert(
            "scaleway.mathieubodin.io/allow-operator-read".to_string(),
            "True".to_string(),
        );
        assert!(!is_opt_in_granted(Some(&labels)));
    }

    #[test]
    fn test_opt_in_denied_when_label_value_yes() {
        let mut labels = BTreeMap::new();
        labels.insert(
            "scaleway.mathieubodin.io/allow-operator-read".to_string(),
            "yes".to_string(),
        );
        assert!(!is_opt_in_granted(Some(&labels)));
    }

    #[test]
    fn test_opt_in_denied_when_label_value_empty_string() {
        let mut labels = BTreeMap::new();
        labels.insert(
            "scaleway.mathieubodin.io/allow-operator-read".to_string(),
            "".to_string(),
        );
        assert!(!is_opt_in_granted(Some(&labels)));
    }

    // ── map_kube_get_error (403/404/autre) ─────────────────────────────────────

    fn api_error(code: u16) -> kube::error::Error {
        use kube::core::Status;
        kube::error::Error::Api(Box::new(Status {
            code,
            message: format!("HTTP {code}"),
            reason: "Test".to_string(),
            ..Default::default()
        }))
    }

    #[test]
    fn test_map_kube_get_error_403_returns_config_error_permanent() {
        let mapped = map_kube_get_error(api_error(403), "db-pass", "team-a");
        let OperatorError::ConfigError(msg) = &mapped else {
            panic!("expected ConfigError, got {mapped:?}");
        };
        assert!(
            msg.contains("RBAC denied"),
            "message should hint at RBAC misconfiguration, got: {msg}"
        );
        assert!(
            msg.contains("team-a"),
            "message should name the namespace, got: {msg}"
        );
        assert!(
            mapped.is_permanent_error(),
            "403 must be permanent (no retry until namespace is bootstrapped)"
        );
    }

    #[test]
    fn test_map_kube_get_error_404_returns_secret_not_found_transient() {
        let mapped = map_kube_get_error(api_error(404), "db-pass", "team-a");
        let OperatorError::SecretNotFound(msg) = &mapped else {
            panic!("expected SecretNotFound, got {mapped:?}");
        };
        assert!(
            msg.contains("db-pass"),
            "message should name the Secret: {msg}"
        );
        assert!(
            msg.contains("team-a"),
            "message should name the namespace: {msg}"
        );
        assert!(
            !mapped.is_permanent_error(),
            "404 must be transient (Secret can appear later)"
        );
    }

    #[test]
    fn test_map_kube_get_error_500_returns_kube_error_transient() {
        let mapped = map_kube_get_error(api_error(500), "x", "ns");
        assert!(matches!(mapped, OperatorError::KubeError(_)));
        assert!(!mapped.is_permanent_error(), "5xx must be transient");
    }

    // ── parse_k8s_secret_source (opt-in + key extraction) ─────────────────────

    fn k8s_secret(
        labels: Option<BTreeMap<String, String>>,
        annotations: Option<BTreeMap<String, String>>,
        data: Option<BTreeMap<String, k8s_openapi::ByteString>>,
        rv: Option<&str>,
    ) -> Secret {
        Secret {
            metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                name: Some("db-pass".to_string()),
                namespace: Some("team-a".to_string()),
                labels,
                annotations,
                resource_version: rv.map(String::from),
                ..Default::default()
            },
            data,
            ..Default::default()
        }
    }

    /// Construit l'annotation `allowed-cr` ciblant `team-a/db-sync` —
    /// le couple CR utilisé dans tous les tests parse_k8s_secret_source_*.
    fn allowed_cr_annotations() -> BTreeMap<String, String> {
        let mut a = BTreeMap::new();
        a.insert(
            ALLOWED_CR_ANNOTATION.to_string(),
            "team-a/db-sync".to_string(),
        );
        a
    }

    #[test]
    fn test_parse_k8s_secret_source_returns_opt_in_missing_when_label_absent() {
        let ks = k8s_secret(None, None, None, Some("12345"));
        let err =
            parse_k8s_secret_source(&ks, "db-pass", "password", "team-a", "db-sync").unwrap_err();
        let OperatorError::SecretOptInMissing(msg) = &err else {
            panic!("expected SecretOptInMissing, got {err:?}");
        };
        assert!(
            msg.contains(OPT_IN_LABEL),
            "message should mention the label key"
        );
        assert!(err.is_permanent_error(), "opt-in missing must be permanent");
    }

    #[test]
    fn test_parse_k8s_secret_source_returns_state_with_payload_on_success() {
        let mut labels = BTreeMap::new();
        labels.insert(OPT_IN_LABEL.to_string(), "true".to_string());
        let mut data = BTreeMap::new();
        data.insert(
            "password".to_string(),
            k8s_openapi::ByteString(b"s3cret".to_vec()),
        );
        let ks = k8s_secret(
            Some(labels),
            Some(allowed_cr_annotations()),
            Some(data),
            Some("12345"),
        );

        let state =
            parse_k8s_secret_source(&ks, "db-pass", "password", "team-a", "db-sync").unwrap();

        assert_eq!(state.resource_version.as_deref(), Some("12345"));
        assert!(state.key_present);
        assert_eq!(state.payload.as_deref(), Some(b"s3cret".as_ref()));
    }

    #[test]
    fn test_parse_k8s_secret_source_returns_state_without_payload_when_key_absent() {
        let mut labels = BTreeMap::new();
        labels.insert(OPT_IN_LABEL.to_string(), "true".to_string());
        let mut data = BTreeMap::new();
        // Le Secret porte une autre clé que celle référencée par le CR.
        data.insert(
            "other_key".to_string(),
            k8s_openapi::ByteString(b"x".to_vec()),
        );
        let ks = k8s_secret(
            Some(labels),
            Some(allowed_cr_annotations()),
            Some(data),
            Some("12345"),
        );

        let state =
            parse_k8s_secret_source(&ks, "db-pass", "password", "team-a", "db-sync").unwrap();

        assert_eq!(state.resource_version.as_deref(), Some("12345"));
        assert!(!state.key_present, "key 'password' is not in .data");
        assert!(state.payload.is_none());
    }

    #[test]
    fn test_parse_k8s_secret_source_no_data_field_at_all() {
        let mut labels = BTreeMap::new();
        labels.insert(OPT_IN_LABEL.to_string(), "true".to_string());
        let ks = k8s_secret(
            Some(labels),
            Some(allowed_cr_annotations()),
            None,
            Some("12345"),
        );

        let state =
            parse_k8s_secret_source(&ks, "db-pass", "password", "team-a", "db-sync").unwrap();

        assert!(!state.key_present);
        assert!(state.payload.is_none());
    }

    // ── build_anonymized_source_error_status (SEC-SECRET-NAME-DISCLOSURE) ────

    #[test]
    fn test_anonymized_status_uses_generic_message() {
        let current = ScalewaySecretStatus::default();
        let s = build_anonymized_source_error_status(&current);

        assert_eq!(
            s.error_message.as_deref(),
            Some(STATUS_ERROR_GENERIC),
            "error_message must be the static generic constant"
        );
        assert_eq!(s.sync_state, "Error");
    }

    #[test]
    fn test_anonymized_status_preserves_unrelated_fields() {
        let current = ScalewaySecretStatus {
            scaleway_id: Some("sec-abc123".to_string()),
            current_version: Some(42),
            last_synced_resource_version: Some("12345".to_string()),
            sync_state: "Synced".to_string(),
            error_message: None,
        };
        let s = build_anonymized_source_error_status(&current);

        assert_eq!(s.scaleway_id, current.scaleway_id);
        assert_eq!(s.current_version, current.current_version);
        assert_eq!(
            s.last_synced_resource_version,
            current.last_synced_resource_version
        );
    }

    #[test]
    fn test_anonymized_status_never_leaks_secret_or_key_name() {
        // Garantie de sécurité : peu importe l'identité du Secret ou de la clé,
        // le status anonymisé ne contient JAMAIS leur nom. Verrouille l'invariant
        // contre une régression qui f-stringerait le nom dans le message.
        let current = ScalewaySecretStatus::default();
        let s = build_anonymized_source_error_status(&current);
        let msg = s.error_message.unwrap();

        for sensitive in &[
            "super-private-db-creds",
            "stripe-api-token",
            "passw0rd",
            "client_secret",
            "tls.key",
        ] {
            assert!(
                !msg.contains(sensitive),
                "status message leaked sensitive token '{sensitive}': {msg}"
            );
        }
    }

    #[test]
    fn test_status_error_generic_is_truly_generic() {
        // Sanity check : la constante elle-même ne contient ni placeholder
        // ni nom de variable qui aurait été oublié dans un format!.
        assert!(
            !STATUS_ERROR_GENERIC.contains('{'),
            "no format placeholder allowed: {STATUS_ERROR_GENERIC}"
        );
        assert!(
            !STATUS_ERROR_GENERIC.contains('\''),
            "single quotes typically wrap injected names: {STATUS_ERROR_GENERIC}"
        );
        assert!(!STATUS_ERROR_GENERIC.is_empty());
    }

    #[test]
    fn test_parse_k8s_secret_source_opt_in_checked_before_key_extraction() {
        // Garantie de sécurité : même si data contient la clé, l'opt-in absent
        // doit empêcher l'extraction de payload (le payload ne doit JAMAIS être lu
        // si l'opt-in n'est pas accordé).
        let mut data = BTreeMap::new();
        data.insert(
            "password".to_string(),
            k8s_openapi::ByteString(b"s3cret".to_vec()),
        );
        let ks = k8s_secret(None, None, Some(data), Some("12345"));

        let err =
            parse_k8s_secret_source(&ks, "db-pass", "password", "team-a", "db-sync").unwrap_err();
        assert!(matches!(err, OperatorError::SecretOptInMissing(_)));
    }

    // ── is_cr_allowed_for_secret (annotation must bind to identity) ───────────

    #[test]
    fn test_is_cr_allowed_for_secret_matches_namespace_name() {
        let mut a = BTreeMap::new();
        a.insert(
            ALLOWED_CR_ANNOTATION.to_string(),
            "team-a/db-sync".to_string(),
        );
        assert!(is_cr_allowed_for_secret(Some(&a), "team-a", "db-sync"));
    }

    #[test]
    fn test_is_cr_allowed_no_annotation() {
        assert!(!is_cr_allowed_for_secret(None, "team-a", "db-sync"));
    }

    #[test]
    fn test_is_cr_allowed_annotation_absent() {
        let mut a = BTreeMap::new();
        a.insert("some.other/annotation".to_string(), "value".to_string());
        assert!(!is_cr_allowed_for_secret(Some(&a), "team-a", "db-sync"));
    }

    #[test]
    fn test_is_cr_allowed_wrong_cr_name() {
        let mut a = BTreeMap::new();
        a.insert(
            ALLOWED_CR_ANNOTATION.to_string(),
            "team-a/other-cr".to_string(),
        );
        assert!(!is_cr_allowed_for_secret(Some(&a), "team-a", "db-sync"));
    }

    #[test]
    fn test_is_cr_allowed_wrong_namespace() {
        // Si on ne comparait que le nom du CR, on ouvrirait un trou cross-namespace.
        // Cette assertion verrouille la comparaison stricte ns + name.
        let mut a = BTreeMap::new();
        a.insert(
            ALLOWED_CR_ANNOTATION.to_string(),
            "team-b/db-sync".to_string(),
        );
        assert!(!is_cr_allowed_for_secret(Some(&a), "team-a", "db-sync"));
    }

    #[test]
    fn test_is_cr_allowed_extra_whitespace() {
        // Match strict : pas de trim, pas de tolérance — un espace parasite est
        // un refus. Évite les surprises où une copie/colle insèrerait un blanc.
        let mut a = BTreeMap::new();
        a.insert(
            ALLOWED_CR_ANNOTATION.to_string(),
            " team-a/db-sync ".to_string(),
        );
        assert!(!is_cr_allowed_for_secret(Some(&a), "team-a", "db-sync"));
    }

    #[test]
    fn test_parse_k8s_secret_source_returns_opt_in_missing_when_cr_annotation_absent() {
        // Label opt-in présent mais annotation `allowed-cr` absente :
        // doit refuser la lecture (label-bypass via patch secrets sans annotation).
        let mut labels = BTreeMap::new();
        labels.insert(OPT_IN_LABEL.to_string(), "true".to_string());
        let mut data = BTreeMap::new();
        data.insert(
            "password".to_string(),
            k8s_openapi::ByteString(b"s3cret".to_vec()),
        );
        let ks = k8s_secret(Some(labels), None, Some(data), Some("12345"));

        let err =
            parse_k8s_secret_source(&ks, "db-pass", "password", "team-a", "db-sync").unwrap_err();
        let OperatorError::SecretOptInMissing(msg) = &err else {
            panic!("expected SecretOptInMissing, got {err:?}");
        };
        assert!(
            msg.contains(ALLOWED_CR_ANNOTATION),
            "message should mention the annotation key: {msg}"
        );
        assert!(err.is_permanent_error());
    }

    #[test]
    fn test_parse_k8s_secret_source_returns_opt_in_missing_when_cr_annotation_mismatches() {
        // Label opt-in présent, annotation présente mais pointe vers un autre CR :
        // doit refuser (le Secret est lié à un autre ScalewaySecret).
        let mut labels = BTreeMap::new();
        labels.insert(OPT_IN_LABEL.to_string(), "true".to_string());
        let mut annotations = BTreeMap::new();
        annotations.insert(
            ALLOWED_CR_ANNOTATION.to_string(),
            "team-a/other-cr".to_string(),
        );
        let mut data = BTreeMap::new();
        data.insert(
            "password".to_string(),
            k8s_openapi::ByteString(b"s3cret".to_vec()),
        );
        let ks = k8s_secret(Some(labels), Some(annotations), Some(data), Some("12345"));

        let err =
            parse_k8s_secret_source(&ks, "db-pass", "password", "team-a", "db-sync").unwrap_err();
        let OperatorError::SecretOptInMissing(msg) = &err else {
            panic!("expected SecretOptInMissing, got {err:?}");
        };
        assert!(
            msg.contains(ALLOWED_CR_ANNOTATION),
            "message should mention the annotation key: {msg}"
        );
        assert!(err.is_permanent_error());
    }

    #[test]
    fn test_parse_k8s_secret_source_success_requires_both_label_and_annotation() {
        // Les deux conditions (label + annotation) sont présentes :
        // le payload est extrait normalement.
        let mut labels = BTreeMap::new();
        labels.insert(OPT_IN_LABEL.to_string(), "true".to_string());
        let mut data = BTreeMap::new();
        data.insert(
            "password".to_string(),
            k8s_openapi::ByteString(b"s3cret".to_vec()),
        );
        let ks = k8s_secret(
            Some(labels),
            Some(allowed_cr_annotations()),
            Some(data),
            Some("12345"),
        );

        let state =
            parse_k8s_secret_source(&ks, "db-pass", "password", "team-a", "db-sync").unwrap();

        assert!(state.key_present);
        assert_eq!(state.payload.as_deref(), Some(b"s3cret".as_ref()));
    }

    #[test]
    fn test_secret_decide_ks_key_not_found_returns_distinct_error() {
        // K8s Secret présent (resource_version Some) mais clé absente de .data —
        // distinct du cas "Secret absent" pour donner un message d'erreur clair.
        let input = SecretReconcileInput {
            current_key_present: false,
            ..base_secret_input()
        };
        assert!(matches!(
            decide_next_action_secret(&input),
            SecretReconcileDecision::ErrorKsKeyNotFound
        ));
    }

    #[test]
    fn test_secret_decide_no_scaleway_id_creates() {
        let input = SecretReconcileInput {
            scaleway_id: None,
            last_synced_resource_version: None,
            ..base_secret_input()
        };
        assert!(matches!(
            decide_next_action_secret(&input),
            SecretReconcileDecision::CreateAndSyncSecret
        ));
    }

    #[test]
    fn test_secret_decide_source_changed_pushes_new_version() {
        let input = SecretReconcileInput {
            current_resource_version: Some("67890".to_string()),
            ..base_secret_input()
        };
        match decide_next_action_secret(&input) {
            SecretReconcileDecision::PushNewVersion { scaleway_id } => {
                assert_eq!(scaleway_id, "sec-abc123");
            }
            other => panic!("expected PushNewVersion, got {other:?}"),
        }
    }

    #[test]
    fn test_secret_decide_deletion_with_scaleway_id_deletes() {
        let input = SecretReconcileInput {
            deletion_requested: true,
            ..base_secret_input()
        };
        assert!(matches!(
            decide_next_action_secret(&input),
            SecretReconcileDecision::DeleteSecret
        ));
    }

    #[test]
    fn test_secret_decide_deletion_without_scaleway_id_removes_finalizer() {
        let input = SecretReconcileInput {
            deletion_requested: true,
            scaleway_id: None,
            ..base_secret_input()
        };
        assert!(matches!(
            decide_next_action_secret(&input),
            SecretReconcileDecision::RemoveFinalizer
        ));
    }

    #[test]
    fn test_secret_decide_deletion_takes_priority_over_circuit_open() {
        let input = SecretReconcileInput {
            deletion_requested: true,
            circuit_open: true,
            ..base_secret_input()
        };
        // Suppression prioritaire sur circuit breaker
        assert!(matches!(
            decide_next_action_secret(&input),
            SecretReconcileDecision::DeleteSecret
        ));
    }
}
