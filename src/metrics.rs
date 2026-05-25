use std::fmt;

use prometheus::{GaugeVec, HistogramOpts, HistogramVec, IntCounterVec, Opts, Registry};

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

impl ReconcileOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReconcileOutcome::Created => "Created",
            ReconcileOutcome::Synced => "Synced",
            ReconcileOutcome::Adopted => "Adopted",
            ReconcileOutcome::Deleted => "Deleted",
            ReconcileOutcome::Error => "Error",
        }
    }
}

impl fmt::Display for ReconcileOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Prometheus metrics for the Scaleway operator.
///
/// `OperatorMetrics` is `Send + Sync` because both `IntCounterVec` and `HistogramVec`
/// from the `prometheus` crate implement those traits.
pub struct OperatorMetrics {
    pub(crate) reconcile_errors_total: IntCounterVec,
    pub(crate) reconcile_duration_seconds: HistogramVec,
    pub(crate) instances_total: GaugeVec,
    pub(crate) load_balancers_total: GaugeVec,
}

impl OperatorMetrics {
    /// Buckets matching the observability plan: 0.1 s, 0.5 s, 1 s, 5 s, 15 s, 30 s.
    const DURATION_BUCKETS: &'static [f64] = &[0.1, 0.5, 1.0, 5.0, 15.0, 30.0];

    /// Build and register all metrics into `registry`.
    ///
    /// Returns an error if any metric is already registered (e.g., in tests that
    /// accidentally call `new` twice on the same registry).
    pub fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let errors_opts = Opts::new(
            "scaleway_operator_reconcile_errors_total",
            "Total number of reconcile errors, labelled by error variant",
        );
        let reconcile_errors_total = IntCounterVec::new(errors_opts, &["error_variant"])?;
        registry.register(Box::new(reconcile_errors_total.clone()))?;

        let duration_opts = HistogramOpts::new(
            "scaleway_operator_reconcile_duration_seconds",
            "Duration of reconcile loop iterations in seconds",
        )
        .buckets(Self::DURATION_BUCKETS.to_vec());
        let reconcile_duration_seconds = HistogramVec::new(duration_opts, &["outcome"])?;
        registry.register(Box::new(reconcile_duration_seconds.clone()))?;

        let instances_opts = Opts::new(
            "scaleway_operator_instances_total",
            "Number of Scaleway instances currently managed by the operator, by zone, type and state",
        );
        let instances_total = GaugeVec::new(instances_opts, &["zone", "instance_type", "state"])?;
        registry.register(Box::new(instances_total.clone()))?;

        let lb_opts = Opts::new(
            "scaleway_operator_load_balancers_total",
            "Number of Scaleway load balancers currently managed by the operator, by zone, type and state",
        );
        let load_balancers_total = GaugeVec::new(lb_opts, &["zone", "lb_type", "state"])?;
        registry.register(Box::new(load_balancers_total.clone()))?;

        Ok(Self {
            reconcile_errors_total,
            reconcile_duration_seconds,
            instances_total,
            load_balancers_total,
        })
    }

    /// Increment the error counter using the error's metric label.
    pub fn record_error(&self, error: &OperatorError) {
        self.reconcile_errors_total
            .with_label_values(&[error.metric_label()])
            .inc();
    }

    /// Observe a reconcile duration for the given outcome.
    pub fn record_duration(&self, outcome: &ReconcileOutcome, duration_secs: f64) {
        self.reconcile_duration_seconds
            .with_label_values(&[outcome.as_str()])
            .observe(duration_secs);
    }

    /// Increment the instances gauge for a (zone, instance_type, state) tuple.
    pub fn inc_instances(&self, zone: &str, instance_type: &str, state: &str) {
        self.instances_total
            .with_label_values(&[zone, instance_type, state])
            .inc();
    }

    /// Decrement the instances gauge for a (zone, instance_type, state) tuple.
    /// Note: prometheus GaugeVec accepts negative values — a decrement without a
    /// prior increment (e.g., after an operator restart) produces -1.0, which is
    /// observable but not blocking. The gauge self-corrects on the next reconcile cycle.
    pub fn dec_instances(&self, zone: &str, instance_type: &str, state: &str) {
        self.instances_total
            .with_label_values(&[zone, instance_type, state])
            .dec();
    }

    /// Increment the load balancers gauge for a (zone, lb_type, state) tuple.
    pub fn inc_load_balancers(&self, zone: &str, lb_type: &str, state: &str) {
        self.load_balancers_total
            .with_label_values(&[zone, lb_type, state])
            .inc();
    }

    /// Decrement the load balancers gauge for a (zone, lb_type, state) tuple.
    pub fn dec_load_balancers(&self, zone: &str, lb_type: &str, state: &str) {
        self.load_balancers_total
            .with_label_values(&[zone, lb_type, state])
            .dec();
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
        assert!(
            metrics.is_ok(),
            "registration should succeed: {:?}",
            metrics.err()
        );
    }

    #[test]
    fn test_new_registers_four_metrics() {
        // prometheus::Registry::gather() prunes empty MetricFamilies (no label values touched yet),
        // so we verify registration via the Collector descriptors instead.
        let registry = fresh_registry();
        let metrics = OperatorMetrics::new(&registry).unwrap();

        use prometheus::core::Collector;
        let counter_descs = metrics.reconcile_errors_total.desc();
        let histogram_descs = metrics.reconcile_duration_seconds.desc();
        let instance_gauge_descs = metrics.instances_total.desc();
        let lb_gauge_descs = metrics.load_balancers_total.desc();

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
        assert_eq!(instance_gauge_descs.len(), 1);
        assert_eq!(
            instance_gauge_descs[0].fq_name,
            "scaleway_operator_instances_total"
        );
        assert_eq!(lb_gauge_descs.len(), 1);
        assert_eq!(
            lb_gauge_descs[0].fq_name,
            "scaleway_operator_load_balancers_total"
        );
    }

    fn gauge_value(metrics: &OperatorMetrics, zone: &str, instance_type: &str, state: &str) -> f64 {
        metrics
            .instances_total
            .with_label_values(&[zone, instance_type, state])
            .get()
    }

    #[test]
    fn test_inc_instances_increments_to_one() {
        let metrics = OperatorMetrics::new(&fresh_registry()).unwrap();
        metrics.inc_instances("fr-par-1", "DEV1-S", "running");
        assert_eq!(gauge_value(&metrics, "fr-par-1", "DEV1-S", "running"), 1.0);
    }

    #[test]
    fn test_inc_instances_twice_reaches_two() {
        let metrics = OperatorMetrics::new(&fresh_registry()).unwrap();
        metrics.inc_instances("fr-par-1", "DEV1-S", "running");
        metrics.inc_instances("fr-par-1", "DEV1-S", "running");
        assert_eq!(gauge_value(&metrics, "fr-par-1", "DEV1-S", "running"), 2.0);
    }

    #[test]
    fn test_dec_instances_after_inc_reaches_zero() {
        let metrics = OperatorMetrics::new(&fresh_registry()).unwrap();
        metrics.inc_instances("fr-par-1", "DEV1-S", "running");
        metrics.dec_instances("fr-par-1", "DEV1-S", "running");
        assert_eq!(gauge_value(&metrics, "fr-par-1", "DEV1-S", "running"), 0.0);
    }

    #[test]
    fn test_dec_instances_without_prior_inc_is_negative() {
        // GaugeVec accepts negative values — a decrement without a prior increment
        // (e.g., after an operator restart during a reconcile cycle) produces -1.0.
        // This is intentional: the value self-corrects on the next reconcile cycle.
        let metrics = OperatorMetrics::new(&fresh_registry()).unwrap();
        metrics.dec_instances("fr-par-1", "DEV1-S", "running");
        assert_eq!(gauge_value(&metrics, "fr-par-1", "DEV1-S", "running"), -1.0);
    }

    #[test]
    fn test_inc_dec_are_label_scoped() {
        let metrics = OperatorMetrics::new(&fresh_registry()).unwrap();
        metrics.inc_instances("fr-par-1", "DEV1-S", "running");
        metrics.inc_instances("nl-ams-1", "GP1-M", "stopped");
        assert_eq!(gauge_value(&metrics, "fr-par-1", "DEV1-S", "running"), 1.0);
        assert_eq!(gauge_value(&metrics, "nl-ams-1", "GP1-M", "stopped"), 1.0);
        assert_eq!(gauge_value(&metrics, "fr-par-1", "DEV1-S", "stopped"), 0.0);
    }

    fn lb_gauge_value(metrics: &OperatorMetrics, zone: &str, lb_type: &str, state: &str) -> f64 {
        metrics
            .load_balancers_total
            .with_label_values(&[zone, lb_type, state])
            .get()
    }

    #[test]
    fn test_inc_load_balancers_increments_to_one() {
        let metrics = OperatorMetrics::new(&fresh_registry()).unwrap();
        metrics.inc_load_balancers("fr-par-1", "LB-S", "ready");
        assert_eq!(lb_gauge_value(&metrics, "fr-par-1", "LB-S", "ready"), 1.0);
    }

    #[test]
    fn test_dec_load_balancers_after_inc_reaches_zero() {
        let metrics = OperatorMetrics::new(&fresh_registry()).unwrap();
        metrics.inc_load_balancers("fr-par-1", "LB-S", "ready");
        metrics.dec_load_balancers("fr-par-1", "LB-S", "ready");
        assert_eq!(lb_gauge_value(&metrics, "fr-par-1", "LB-S", "ready"), 0.0);
    }

    #[test]
    fn test_lb_gauge_is_label_scoped() {
        let metrics = OperatorMetrics::new(&fresh_registry()).unwrap();
        metrics.inc_load_balancers("fr-par-1", "LB-S", "ready");
        metrics.inc_load_balancers("nl-ams-1", "LB-GP", "pending");
        assert_eq!(lb_gauge_value(&metrics, "fr-par-1", "LB-S", "ready"), 1.0);
        assert_eq!(
            lb_gauge_value(&metrics, "nl-ams-1", "LB-GP", "pending"),
            1.0
        );
        assert_eq!(lb_gauge_value(&metrics, "fr-par-1", "LB-S", "pending"), 0.0);
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
        let err = OperatorError::KubeError(kube::Error::SerdeError(serde_err.unwrap_err()));
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
    fn test_record_error_serialization_error() {
        let registry = fresh_registry();
        let metrics = OperatorMetrics::new(&registry).unwrap();
        let err = OperatorError::SerializationError(
            serde_json::from_str::<serde_json::Value>("not json").unwrap_err(),
        );
        metrics.record_error(&err);
        assert_counter_value(&metrics, "SerializationError", 1);
    }

    #[test]
    fn test_record_error_network_error() {
        // reqwest::Error via an invalid URL scheme — no network call, fails at request build time.
        let registry = fresh_registry();
        let metrics = OperatorMetrics::new(&registry).unwrap();
        let reqwest_err = reqwest::blocking::get("not-a-valid-url://host").unwrap_err();
        let err = OperatorError::NetworkError(reqwest_err);
        metrics.record_error(&err);
        assert_counter_value(&metrics, "NetworkError", 1);
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
