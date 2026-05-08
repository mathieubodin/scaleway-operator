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

    #[error("Invalid zone: {0}")]
    InvalidZone(String),

    #[error("Invalid instance type: {0}")]
    InvalidInstanceType(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[allow(dead_code)]
    #[error("Finalization error: {0}")]
    FinalizationError(String),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

pub type Result<T> = std::result::Result<T, OperatorError>;

impl OperatorError {
    /// Returns a sanitized message suitable for writing to CRD status.
    /// For ScalewayError, extracts only the "message" field from the JSON body
    /// to avoid exposing resource IDs or raw API response data to namespace readers.
    /// Full error detail is preserved in to_string() for tracing/logging.
    pub fn for_status(&self) -> String {
        match self {
            OperatorError::ScalewayError { status, message } => {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(message) {
                    if let Some(msg) = json.get("message").and_then(|v| v.as_str()) {
                        return format!("Scaleway API error: {} — {}", status, msg);
                    }
                }
                format!("Scaleway API error: {}", status)
            }
            _ => self.to_string(),
        }
    }
}
