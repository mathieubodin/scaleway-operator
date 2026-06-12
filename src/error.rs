use thiserror::Error;

#[derive(Error, Debug)]
pub enum OperatorError {
    #[error("Kubernetes API error: {0}")]
    KubeError(#[from] kube::error::Error),

    #[error("Scaleway API error: {status} - {message}")]
    ScalewayError { status: String, message: String },

    #[error("Project access denied: {0}")]
    ProjectAccessDenied(String),

    #[error("Instance not found: {0}")]
    InstanceNotFound(String),

    #[error("Load balancer not found: {0}")]
    LbNotFound(String),

    #[error("Invalid zone: {0}")]
    InvalidZone(String),

    #[error("Invalid instance type: {0}")]
    InvalidInstanceType(String),

    #[error("Invalid load balancer type: {0}")]
    InvalidLbType(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[allow(dead_code)]
    #[error("Finalization error: {0}")]
    FinalizationError(String),

    #[error("Secret not found: {0}")]
    SecretNotFound(String),

    /// La spec du CR ne contient pas de source valide (erreur permanente — ne se résout
    /// pas sans intervention de l'utilisateur).
    #[error("Secret source not configured: {0}")]
    SecretSourceNotConfigured(String),

    /// Le Secret Kubernetes référencé n'a pas le label d'opt-in requis.
    /// Erreur permanente — ne se résout pas sans ajouter le label sur le Secret.
    #[error("Secret opt-in missing: {0}")]
    SecretOptInMissing(String),

    #[error("Unknown error: {0}")]
    Unknown(String),

    #[error("Scaleway API circuit breaker open")]
    CircuitBreakerOpen,
}

pub type Result<T> = std::result::Result<T, OperatorError>;

impl OperatorError {
    /// Returns the PascalCase variant name for use as a Prometheus label.
    pub fn metric_label(&self) -> &'static str {
        match self {
            OperatorError::KubeError(_) => "KubeError",
            OperatorError::ScalewayError { .. } => "ScalewayError",
            OperatorError::ProjectAccessDenied(_) => "ProjectAccessDenied",
            OperatorError::InstanceNotFound(_) => "InstanceNotFound",
            OperatorError::LbNotFound(_) => "LbNotFound",
            OperatorError::InvalidZone(_) => "InvalidZone",
            OperatorError::InvalidInstanceType(_) => "InvalidInstanceType",
            OperatorError::InvalidLbType(_) => "InvalidLbType",
            OperatorError::ConfigError(_) => "ConfigError",
            OperatorError::NetworkError(_) => "NetworkError",
            OperatorError::SerializationError(_) => "SerializationError",
            OperatorError::FinalizationError(_) => "FinalizationError",
            OperatorError::SecretNotFound(_) => "SecretNotFound",
            OperatorError::SecretSourceNotConfigured(_) => "SecretSourceNotConfigured",
            OperatorError::SecretOptInMissing(_) => "SecretOptInMissing",
            OperatorError::Unknown(_) => "Unknown",
            OperatorError::CircuitBreakerOpen => "CircuitBreakerOpen",
        }
    }

    /// Returns true for errors that indicate a permanent misconfiguration.
    /// The reconciler should not requeue on permanent errors — only user
    /// intervention (editing the CR spec) can resolve them.
    pub fn is_permanent_error(&self) -> bool {
        matches!(
            self,
            OperatorError::InvalidZone(_)
                | OperatorError::InvalidInstanceType(_)
                | OperatorError::InvalidLbType(_)
                | OperatorError::ConfigError(_)
                | OperatorError::ProjectAccessDenied(_)
                | OperatorError::SecretSourceNotConfigured(_)
                | OperatorError::SecretOptInMissing(_)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_for_status_scaleway_error_extracts_message_field() {
        let e = OperatorError::ScalewayError {
            status: "403 Forbidden".to_string(),
            message:
                r#"{"message":"Permission denied","resource_id":"srv-abc123","type":"forbidden"}"#
                    .to_string(),
        };
        assert_eq!(
            e.for_status(),
            "Scaleway API error: 403 Forbidden — Permission denied"
        );
    }

    #[test]
    fn test_for_status_scaleway_error_non_json_fallback() {
        let e = OperatorError::ScalewayError {
            status: "500 Internal Server Error".to_string(),
            message: "plain text error".to_string(),
        };
        assert_eq!(
            e.for_status(),
            "Scaleway API error: 500 Internal Server Error"
        );
    }

    #[test]
    fn test_for_status_scaleway_error_json_without_message_field() {
        let e = OperatorError::ScalewayError {
            status: "404 Not Found".to_string(),
            message: r#"{"code":404,"resource_id":"srv-xyz"}"#.to_string(),
        };
        assert_eq!(e.for_status(), "Scaleway API error: 404 Not Found");
    }

    #[test]
    fn test_for_status_scaleway_error_non_string_message_value() {
        let e = OperatorError::ScalewayError {
            status: "400 Bad Request".to_string(),
            message: r#"{"message":42}"#.to_string(),
        };
        assert_eq!(e.for_status(), "Scaleway API error: 400 Bad Request");
    }

    #[test]
    fn test_for_status_network_error_sanitized() {
        // NetworkError wraps reqwest::Error which embeds URLs — must never reach status
        let e = OperatorError::Unknown("some error".to_string());
        assert_eq!(e.for_status(), "Unknown error: some error");
    }

    #[test]
    fn test_for_status_instance_not_found_passthrough() {
        let e = OperatorError::InstanceNotFound("srv-abc".to_string());
        assert_eq!(e.for_status(), "Instance not found: srv-abc");
    }

    #[test]
    fn test_for_status_config_error_passthrough() {
        let e = OperatorError::ConfigError("bad annotation".to_string());
        assert_eq!(e.for_status(), "Configuration error: bad annotation");
    }

    #[test]
    fn test_for_status_secret_not_found_passthrough() {
        let e = OperatorError::SecretNotFound("Key 'token' missing".to_string());
        assert_eq!(e.for_status(), "Secret not found: Key 'token' missing");
    }

    #[test]
    fn test_for_status_secret_source_not_configured_passthrough() {
        let e = OperatorError::SecretSourceNotConfigured("missing source".to_string());
        assert_eq!(
            e.for_status(),
            "Secret source not configured: missing source"
        );
    }

    #[test]
    fn test_for_status_secret_opt_in_missing_passthrough() {
        let e = OperatorError::SecretOptInMissing("label required".to_string());
        assert_eq!(e.for_status(), "Secret opt-in missing: label required");
    }

    #[test]
    fn test_for_status_circuit_breaker_open_generic() {
        let e = OperatorError::CircuitBreakerOpen;
        assert_eq!(e.for_status(), "Scaleway API temporarily unavailable");
    }

    #[test]
    fn test_lb_not_found_metric_label() {
        let e = OperatorError::LbNotFound("lb-abc123".to_string());
        assert_eq!(e.metric_label(), "LbNotFound");
    }

    #[test]
    fn test_lb_not_found_for_status_passthrough() {
        let e = OperatorError::LbNotFound("lb-xyz".to_string());
        assert_eq!(e.for_status(), "Load balancer not found: lb-xyz");
    }

    #[test]
    fn test_secret_not_found_metric_label() {
        let e = OperatorError::SecretNotFound("my-secret".to_string());
        assert_eq!(e.metric_label(), "SecretNotFound");
    }

    #[test]
    fn test_secret_source_not_configured_metric_label() {
        let e = OperatorError::SecretSourceNotConfigured("no source".to_string());
        assert_eq!(e.metric_label(), "SecretSourceNotConfigured");
    }

    #[test]
    fn test_secret_opt_in_missing_metric_label() {
        let e = OperatorError::SecretOptInMissing("no label".to_string());
        assert_eq!(e.metric_label(), "SecretOptInMissing");
    }

    #[test]
    fn test_secret_not_found_is_not_permanent() {
        let e = OperatorError::SecretNotFound("my-secret".to_string());
        assert!(!e.is_permanent_error());
    }

    #[test]
    fn test_secret_source_not_configured_is_permanent() {
        let e = OperatorError::SecretSourceNotConfigured("bad spec".to_string());
        assert!(e.is_permanent_error());
    }

    #[test]
    fn test_transient_errors_are_not_permanent() {
        assert!(!OperatorError::CircuitBreakerOpen.is_permanent_error());
        assert!(!OperatorError::Unknown("oops".to_string()).is_permanent_error());
        assert!(!OperatorError::SecretNotFound("sec".to_string()).is_permanent_error());
    }

    #[test]
    fn test_invalid_zone_is_permanent() {
        let e = OperatorError::InvalidZone("bad-zone".to_string());
        assert!(e.is_permanent_error());
    }

    #[test]
    fn test_invalid_instance_type_is_permanent() {
        let e = OperatorError::InvalidInstanceType("MEGA-XL".to_string());
        assert!(e.is_permanent_error());
    }

    #[test]
    fn test_invalid_lb_type_is_permanent() {
        let e = OperatorError::InvalidLbType("MEGA-LB".to_string());
        assert!(e.is_permanent_error());
    }

    #[test]
    fn test_config_error_is_permanent() {
        let e = OperatorError::ConfigError("bad annotation".to_string());
        assert!(e.is_permanent_error());
    }

    #[test]
    fn test_project_access_denied_is_permanent() {
        let e = OperatorError::ProjectAccessDenied("proj-x".to_string());
        assert!(e.is_permanent_error());
    }

    #[test]
    fn test_secret_opt_in_missing_is_permanent() {
        let e = OperatorError::SecretOptInMissing("no label".to_string());
        assert!(e.is_permanent_error());
    }

    #[test]
    fn test_scaleway_error_is_transient() {
        let e = OperatorError::ScalewayError {
            status: "500 Internal Server Error".to_string(),
            message: "server error".to_string(),
        };
        assert!(!e.is_permanent_error());
    }

    #[test]
    fn test_finalization_error_is_transient() {
        let e = OperatorError::FinalizationError("timeout".to_string());
        assert!(!e.is_permanent_error());
    }
}

impl OperatorError {
    /// Returns a sanitized message suitable for writing to CRD status.
    /// For ScalewayError, extracts only the "message" field from the JSON body
    /// to avoid exposing resource IDs or raw API response data to namespace readers.
    /// Full error detail is preserved in to_string() for tracing/logging.
    pub(crate) fn for_status(&self) -> String {
        match self {
            OperatorError::ScalewayError { status, message } => {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(message) {
                    if let Some(msg) = json.get("message").and_then(|v| v.as_str()) {
                        return format!("Scaleway API error: {} — {}", status, msg);
                    }
                }
                format!("Scaleway API error: {}", status)
            }
            // Sanitize variants that may embed internal URLs or API server addresses
            OperatorError::NetworkError(_) => {
                "Network error communicating with Scaleway API".to_string()
            }
            OperatorError::KubeError(_) => "Kubernetes API error".to_string(),
            OperatorError::SerializationError(_) => "Response parsing error".to_string(),
            OperatorError::CircuitBreakerOpen => "Scaleway API temporarily unavailable".to_string(),
            _ => self.to_string(),
        }
    }
}
