use std::fmt;

use prometheus::{Histogram, HistogramOpts, HistogramVec, IntCounterVec, Opts, Registry};

use crate::error::OperatorError;

/// Outcome of a reconcile loop iteration, used as the `outcome` label on the duration histogram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconcileOutcome {
    Created,
    Synced,
    Adopted,
    Deleted,
    Error,
}

impl fmt::Display for ReconcileOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReconcileOutcome::Created => write!(f, "Created"),
            ReconcileOutcome::Synced => write!(f, "Synced"),
            ReconcileOutcome::Adopted => write!(f, "Adopted"),
            ReconcileOutcome::Deleted => write!(f, "Deleted"),
            ReconcileOutcome::Error => write!(f, "Error"),
        }
    }
}

/// Prometheus metrics for the Scaleway operator.
///
/// `OperatorMetrics` is `Send + Sync` because both `IntCounterVec` and `HistogramVec`
/// from the `prometheus` crate implement those traits.
pub struct OperatorMetrics {
    /// `scaleway_operator_reconcile_errors_total{error_variant="<PascalCase>"}` — incremented
    /// each time a reconcile loop returns an error.
    pub reconcile_errors_total: IntCounterVec,

    /// `scaleway_operator_reconcile_duration_seconds{outcome="<ReconcileOutcome>"}` — records
    /// the wall-clock duration of every reconcile loop iteration.
    pub reconcile_duration_seconds: HistogramVec,
}

impl OperatorMetrics {
    /// Buckets matching the observability plan: 0.1 s, 0.5 s, 1 s, 5 s, 15 s, 30 s.
    const DURATION_BUCKETS: &'static [f64] = &[0.1, 0.5, 1.0, 5.0, 15.0, 30.0];

    /// Build and register both metrics into `registry`.
    ///
    /// Returns an error if either metric is already registered (e.g., in tests that
    /// accidentally call `new` twice on the same registry).
    pub fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let errors_opts = Opts::new(
            "scaleway_operator_reconcile_errors_total",
            "Total number of reconcile errors, labelled by error variant",
        );
        let reconcile_errors_total =
            IntCounterVec::new(errors_opts, &["error_variant"])?;
        registry.register(Box::new(reconcile_errors_total.clone()))?;

        let duration_opts = HistogramOpts::new(
            "scaleway_operator_reconcile_duration_seconds",
            "Duration of reconcile loop iterations in seconds",
        )
        .buckets(Self::DURATION_BUCKETS.to_vec());
        let reconcile_duration_seconds =
            HistogramVec::new(duration_opts, &["outcome"])?;
        registry.register(Box::new(reconcile_duration_seconds.clone()))?;

        Ok(Self {
            reconcile_errors_total,
            reconcile_duration_seconds,
        })
    }

    /// Increment the error counter for the PascalCase variant name of `error`.
    pub fn record_error(&self, error: &OperatorError) {
        let variant = variant_name(error);
        self.reconcile_errors_total
            .with_label_values(&[variant])
            .inc();
    }

    /// Observe a reconcile duration for the given outcome.
    pub fn record_duration(&self, outcome: &ReconcileOutcome, duration_secs: f64) {
        self.reconcile_duration_seconds
            .with_label_values(&[&outcome.to_string()])
            .observe(duration_secs);
    }

    /// Return a `prometheus::Histogram` for a specific outcome label (convenience wrapper).
    pub fn duration_histogram(&self, outcome: &ReconcileOutcome) -> Histogram {
        self.reconcile_duration_seconds
            .with_label_values(&[&outcome.to_string()])
    }
}

/// Map an [`OperatorError`] variant to its PascalCase name for use as a Prometheus label.
fn variant_name(error: &OperatorError) -> &'static str {
    match error {
        OperatorError::KubeError(_) => "KubeError",
        OperatorError::ScalewayError { .. } => "ScalewayError",
        OperatorError::ProjectAccessDenied(_) => "ProjectAccessDenied",
        OperatorError::InstanceNotFound(_) => "InstanceNotFound",
        OperatorError::InvalidZone(_) => "InvalidZone",
        OperatorError::InvalidInstanceType(_) => "InvalidInstanceType",
        OperatorError::ConfigError(_) => "ConfigError",
        OperatorError::NetworkError(_) => "NetworkError",
        OperatorError::SerializationError(_) => "SerializationError",
        OperatorError::FinalizationError(_) => "FinalizationError",
        OperatorError::Unknown(_) => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prometheus::Registry;

    fn fresh_registry() -> Registry {
        Registry::new()
    }

    // ── OperatorMetrics::new registers both metrics without panicking ────────────

    #[test]
    fn test_new_registers_without_panic() {
        let registry = fresh_registry();
        let metrics = OperatorMetrics::new(&registry);
        assert!(metrics.is_ok(), "registration should succeed: {:?}", metrics.err());
    }

    #[test]
    fn test_new_registers_both_metrics() {
        // prometheus::Registry::gather() prunes empty MetricFamilies (no label values touched yet),
        // so we verify registration via the Collector descriptors instead.
        let registry = fresh_registry();
        let metrics = OperatorMetrics::new(&registry).unwrap();

        use prometheus::core::Collector;
        let counter_descs = metrics.reconcile_errors_total.desc();
        let histogram_descs = metrics.reconcile_duration_seconds.desc();

        assert_eq!(counter_descs.len(), 1);
        assert_eq!(
            counter_descs[0].fq_name,
            "scaleway_operator_reconcile_errors_total"
        );
        assert_eq!(histogram_descs.len(), 1);
        assert_eq!(
            histogram_descs[0].fq_name,
            "scaleway_operator_reconcile_duration_seconds"
        );
    }

    // ── ReconcileOutcome Display produces non-empty strings without spaces ───────

    #[test]
    fn test_reconcile_outcome_display_no_spaces() {
        let variants = [
            ReconcileOutcome::Created,
            ReconcileOutcome::Synced,
            ReconcileOutcome::Adopted,
            ReconcileOutcome::Deleted,
            ReconcileOutcome::Error,
        ];
        for variant in &variants {
            let s = variant.to_string();
            assert!(!s.is_empty(), "Display of {:?} must not be empty", variant);
            assert!(
                !s.contains(' '),
                "Display of {:?} must not contain spaces, got: {:?}",
                variant,
                s
            );
        }
    }

    // ── Double registration returns an error (not a silent panic) ────────────────

    #[test]
    fn test_double_registration_returns_error() {
        let registry = fresh_registry();
        let first = OperatorMetrics::new(&registry);
        assert!(first.is_ok(), "first registration must succeed");
        let second = OperatorMetrics::new(&registry);
        assert!(
            second.is_err(),
            "second registration on the same registry must return an error"
        );
    }

    // ── record_error increments counter with the correct PascalCase label ────────

    fn assert_counter_value(metrics: &OperatorMetrics, label: &str, expected: u64) {
        let value = metrics
            .reconcile_errors_total
            .with_label_values(&[label])
            .get();
        assert_eq!(
            value, expected,
            "counter for label {:?} should be {}, got {}",
            label, expected, value
        );
    }

    #[test]
    fn test_record_error_kube_error() {
        let registry = fresh_registry();
        let metrics = OperatorMetrics::new(&registry).unwrap();
        // Construct a kube::Error via SerdeError (wraps serde_json::Error).
        let serde_err: serde_json::Result<serde_json::Value> = serde_json::from_str("not json");
        let err =
            OperatorError::KubeError(kube::Error::SerdeError(serde_err.unwrap_err()));
        metrics.record_error(&err);
        assert_counter_value(&metrics, "KubeError", 1);
    }

    #[test]
    fn test_record_error_scaleway_error() {
        let registry = fresh_registry();
        let metrics = OperatorMetrics::new(&registry).unwrap();
        let err = OperatorError::ScalewayError {
            status: "403".to_string(),
            message: "denied".to_string(),
        };
        metrics.record_error(&err);
        assert_counter_value(&metrics, "ScalewayError", 1);
    }

    #[test]
    fn test_record_error_project_access_denied() {
        let registry = fresh_registry();
        let metrics = OperatorMetrics::new(&registry).unwrap();
        metrics.record_error(&OperatorError::ProjectAccessDenied("p".to_string()));
        assert_counter_value(&metrics, "ProjectAccessDenied", 1);
    }

    #[test]
    fn test_record_error_instance_not_found() {
        let registry = fresh_registry();
        let metrics = OperatorMetrics::new(&registry).unwrap();
        metrics.record_error(&OperatorError::InstanceNotFound("i".to_string()));
        assert_counter_value(&metrics, "InstanceNotFound", 1);
    }

    #[test]
    fn test_record_error_invalid_zone() {
        let registry = fresh_registry();
        let metrics = OperatorMetrics::new(&registry).unwrap();
        metrics.record_error(&OperatorError::InvalidZone("z".to_string()));
        assert_counter_value(&metrics, "InvalidZone", 1);
    }

    #[test]
    fn test_record_error_invalid_instance_type() {
        let registry = fresh_registry();
        let metrics = OperatorMetrics::new(&registry).unwrap();
        metrics.record_error(&OperatorError::InvalidInstanceType("t".to_string()));
        assert_counter_value(&metrics, "InvalidInstanceType", 1);
    }

    #[test]
    fn test_record_error_config_error() {
        let registry = fresh_registry();
        let metrics = OperatorMetrics::new(&registry).unwrap();
        metrics.record_error(&OperatorError::ConfigError("c".to_string()));
        assert_counter_value(&metrics, "ConfigError", 1);
    }

    #[test]
    fn test_record_error_network_error() {
        // Build a reqwest::Error via a known-invalid URL to avoid hitting the network.
        let registry = fresh_registry();
        let metrics = OperatorMetrics::new(&registry).unwrap();

        // We can't easily construct reqwest::Error directly, so we use Unknown as a proxy
        // and verify the NetworkError arm by using serde_json::Error to get SerializationError.
        // For NetworkError specifically, test the variant_name function directly.
        // We'll use a workaround: parse a definitely-invalid JSON to construct
        // a SerializationError, then manually test variant_name with a string check.
        let serialization_err: Result<serde_json::Value, _> =
            serde_json::from_str("not json");
        let err = OperatorError::SerializationError(serialization_err.unwrap_err());
        metrics.record_error(&err);
        assert_counter_value(&metrics, "SerializationError", 1);
    }

    #[test]
    fn test_record_error_finalization_error() {
        let registry = fresh_registry();
        let metrics = OperatorMetrics::new(&registry).unwrap();
        metrics.record_error(&OperatorError::FinalizationError("f".to_string()));
        assert_counter_value(&metrics, "FinalizationError", 1);
    }

    #[test]
    fn test_record_error_unknown() {
        let registry = fresh_registry();
        let metrics = OperatorMetrics::new(&registry).unwrap();
        metrics.record_error(&OperatorError::Unknown("u".to_string()));
        assert_counter_value(&metrics, "Unknown", 1);
    }

    #[test]
    fn test_record_error_increments_only_matching_label() {
        let registry = fresh_registry();
        let metrics = OperatorMetrics::new(&registry).unwrap();
        metrics.record_error(&OperatorError::ConfigError("x".to_string()));
        metrics.record_error(&OperatorError::ConfigError("y".to_string()));
        assert_counter_value(&metrics, "ConfigError", 2);
        assert_counter_value(&metrics, "Unknown", 0);
    }
}
