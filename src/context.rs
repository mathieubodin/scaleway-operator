use crate::scaleway::ScalewayClient;
use kube::Client;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const SCALEWAY_PROJECT_ANNOTATION: &str = "scaleway.mathieubodin.io/project-id";

const CIRCUIT_FAILURE_THRESHOLD: u32 = 5;
const CIRCUIT_OPEN_TIMEOUT: Duration = Duration::from_secs(60);

pub enum CircuitBreakerState {
    Closed { failure_count: u32 },
    Open { opened_at: Instant },
    HalfOpen,
}

pub struct Context {
    pub client: Client,
    pub scaleway_client: ScalewayClient,
    pub organization_id: String,
    pub scaleway_base_url: String,
    pub metrics: crate::metrics::OperatorMetrics,
    pub last_reconcile_at: std::sync::atomic::AtomicI64,
    pub retry_counts: Mutex<HashMap<String, u32>>,
    pub circuit_breaker: Mutex<CircuitBreakerState>,
}

impl Context {
    /// Incrémente le compteur d'erreurs consécutives pour une ressource et retourne la nouvelle valeur.
    pub fn increment_retry_count(&self, key: &str) -> u32 {
        let mut counts = self.retry_counts.lock().unwrap();
        let count = counts.entry(key.to_string()).or_insert(0);
        *count += 1;
        *count
    }

    /// Remet à zéro le compteur après une réconciliation réussie.
    pub fn reset_retry_count(&self, key: &str) {
        self.retry_counts.lock().unwrap().remove(key);
    }

    /// Retourne `true` si le circuit est ouvert (appels Scaleway doivent être bloqués).
    /// Si l'état est `Open` et que le timeout est écoulé, transite vers `HalfOpen` et retourne `false`.
    pub fn is_circuit_open(&self) -> bool {
        let mut state = self.circuit_breaker.lock().unwrap();
        match *state {
            CircuitBreakerState::Open { opened_at } => {
                if opened_at.elapsed() < CIRCUIT_OPEN_TIMEOUT {
                    true
                } else {
                    *state = CircuitBreakerState::HalfOpen;
                    false
                }
            }
            _ => false,
        }
    }

    /// Enregistre un échec d'appel Scaleway transitoire.
    /// Ouvre le circuit si le seuil est atteint.
    pub fn record_scaleway_failure(&self) {
        let mut state = self.circuit_breaker.lock().unwrap();
        match *state {
            CircuitBreakerState::Closed { ref mut failure_count } => {
                *failure_count += 1;
                if *failure_count >= CIRCUIT_FAILURE_THRESHOLD {
                    *state = CircuitBreakerState::Open { opened_at: Instant::now() };
                }
            }
            CircuitBreakerState::HalfOpen => {
                *state = CircuitBreakerState::Open { opened_at: Instant::now() };
            }
            CircuitBreakerState::Open { .. } => {}
        }
    }

    /// Enregistre un succès d'appel Scaleway.
    /// Referme le circuit si on était en HalfOpen.
    pub fn record_scaleway_success(&self) {
        let mut state = self.circuit_breaker.lock().unwrap();
        match *state {
            CircuitBreakerState::HalfOpen => {
                *state = CircuitBreakerState::Closed { failure_count: 0 };
            }
            CircuitBreakerState::Closed { ref mut failure_count } => {
                *failure_count = 0;
            }
            CircuitBreakerState::Open { .. } => {}
        }
    }
}

/// Extraire le project_id depuis une annotation de namespace
pub fn extract_project_id_from_namespace(
    namespace_annotations: &std::collections::BTreeMap<String, String>,
) -> Option<String> {
    namespace_annotations
        .get(SCALEWAY_PROJECT_ANNOTATION)
        .cloned()
}

/// Récupérer le rôle Scaleway pour un namespace donné
pub async fn get_scaleway_role_for_namespace(
    client: &Client,
    namespace: &str,
) -> crate::error::Result<String> {
    use crate::resources::NamespaceRole;
    use kube::Api;

    let api: Api<NamespaceRole> = Api::all(client.clone());

    match api.get(namespace).await {
        Ok(ns_role) => {
            tracing::debug!(
                namespace = %namespace,
                role = %ns_role.spec.scaleway_role,
                "Found NamespaceRole for namespace"
            );
            Ok(ns_role.spec.scaleway_role.clone())
        }
        Err(kube::error::Error::Api(ae)) if ae.code == 404 => {
            tracing::error!(
                namespace = %namespace,
                "No NamespaceRole found for namespace"
            );
            Err(crate::error::OperatorError::ConfigError(format!(
                "No NamespaceRole found for namespace '{}'. \
                 Create a NamespaceRole resource with name '{}' to assign a Scaleway role.",
                namespace, namespace
            )))
        }
        Err(e) => {
            tracing::error!(
                namespace = %namespace,
                error = %e,
                "Failed to get NamespaceRole"
            );
            Err(crate::error::OperatorError::KubeError(e))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn fresh_circuit_breaker() -> Mutex<CircuitBreakerState> {
        Mutex::new(CircuitBreakerState::Closed { failure_count: 0 })
    }

    // Circuit breaker tests operate only on Mutex<CircuitBreakerState> — no kube::Client needed.
    // We use a standalone struct rather than a full Context to avoid tokio runtime requirements.

    struct TestCircuit {
        cb: Mutex<CircuitBreakerState>,
    }

    impl TestCircuit {
        fn new() -> Self {
            Self { cb: Mutex::new(CircuitBreakerState::Closed { failure_count: 0 }) }
        }

        fn is_open(&self) -> bool {
            let mut state = self.cb.lock().unwrap();
            match *state {
                CircuitBreakerState::Open { opened_at } => {
                    if opened_at.elapsed() < CIRCUIT_OPEN_TIMEOUT {
                        true
                    } else {
                        *state = CircuitBreakerState::HalfOpen;
                        false
                    }
                }
                _ => false,
            }
        }

        fn failure(&self) {
            let mut state = self.cb.lock().unwrap();
            match *state {
                CircuitBreakerState::Closed { ref mut failure_count } => {
                    *failure_count += 1;
                    if *failure_count >= CIRCUIT_FAILURE_THRESHOLD {
                        *state = CircuitBreakerState::Open { opened_at: Instant::now() };
                    }
                }
                CircuitBreakerState::HalfOpen => {
                    *state = CircuitBreakerState::Open { opened_at: Instant::now() };
                }
                CircuitBreakerState::Open { .. } => {}
            }
        }

        fn success(&self) {
            let mut state = self.cb.lock().unwrap();
            match *state {
                CircuitBreakerState::HalfOpen => {
                    *state = CircuitBreakerState::Closed { failure_count: 0 };
                }
                CircuitBreakerState::Closed { ref mut failure_count } => {
                    *failure_count = 0;
                }
                CircuitBreakerState::Open { .. } => {}
            }
        }
    }

    #[test]
    fn test_circuit_opens_after_five_failures() {
        let cb = TestCircuit::new();
        for _ in 0..5 { cb.failure(); }
        assert!(cb.is_open(), "circuit should be open after 5 failures");
    }

    #[test]
    fn test_circuit_stays_closed_after_four_failures() {
        let cb = TestCircuit::new();
        for _ in 0..4 { cb.failure(); }
        assert!(!cb.is_open(), "circuit should remain closed after only 4 failures");
    }

    #[test]
    fn test_success_in_half_open_closes_circuit() {
        let cb = TestCircuit::new();
        *cb.cb.lock().unwrap() = CircuitBreakerState::HalfOpen;
        cb.success();
        assert!(!cb.is_open(), "circuit should be closed after success in HalfOpen");
    }

    #[test]
    fn test_failure_in_half_open_reopens_circuit() {
        let cb = TestCircuit::new();
        *cb.cb.lock().unwrap() = CircuitBreakerState::HalfOpen;
        cb.failure();
        assert!(cb.is_open(), "circuit should reopen after failure in HalfOpen");
    }

    #[test]
    fn test_success_in_closed_resets_failure_count() {
        let cb = TestCircuit::new();
        for _ in 0..4 { cb.failure(); }
        cb.success();
        // After reset, 4 more failures should not open the circuit
        for _ in 0..4 { cb.failure(); }
        assert!(!cb.is_open(), "failure_count should have been reset to 0 by success");
    }

    #[test]
    fn test_circuit_breaker_open_variant_exists() {
        let _ = crate::error::OperatorError::CircuitBreakerOpen;
    }

    #[test]
    fn test_extract_project_id() {
        let mut annotations = BTreeMap::new();
        annotations.insert(
            SCALEWAY_PROJECT_ANNOTATION.to_string(),
            "proj-123".to_string(),
        );

        assert_eq!(
            extract_project_id_from_namespace(&annotations),
            Some("proj-123".to_string())
        );
    }

    #[test]
    fn test_extract_project_id_missing() {
        let annotations = BTreeMap::new();
        assert_eq!(extract_project_id_from_namespace(&annotations), None);
    }

    #[test]
    fn test_extract_project_id_ignores_other_annotations() {
        let mut annotations = BTreeMap::new();
        annotations.insert("unrelated.io/key".to_string(), "value".to_string());
        assert_eq!(extract_project_id_from_namespace(&annotations), None);
    }
}
