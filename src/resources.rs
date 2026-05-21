use chrono::{DateTime, Utc};
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ============== Instance Resource ==============

#[derive(CustomResource, Serialize, Deserialize, Debug, Clone, JsonSchema)]
#[kube(group = "scaleway.mathieubodin.io", version = "v1", kind = "Instance")]
#[kube(namespaced)]
#[kube(status = "InstanceStatus")]
#[kube(printcolumn = r#"{"name":"Scaleway ID","type":"string","jsonPath":".status.scalewayId"}"#)]
#[kube(printcolumn = r#"{"name":"State","type":"string","jsonPath":".status.state"}"#)]
#[kube(printcolumn = r#"{"name":"IP","type":"string","jsonPath":".status.publicIp"}"#)]
pub struct InstanceSpec {
    /// Nom de l'instance Scaleway
    pub name: String,

    /// Zone Scaleway (fr-par-1, nl-ams-1, etc.)
    pub zone: String,

    /// Image à utiliser (ubuntu-jammy, debian-12, etc.)
    pub image: String,

    /// Type commercial (DEV1-S, GP1-M, GPU-3090, etc.)
    pub instance_type: String,

    /// Tags optionnels
    #[serde(default)]
    pub tags: Vec<String>,

    /// Boot volume size en GB (optionnel, défaut: 20)
    #[serde(default = "default_boot_size")]
    pub boot_volume_size: i32,

    /// Configuration réseau optionnelle
    #[serde(default)]
    pub network: Option<NetworkConfig>,

    /// Configuration de sécurité optionnelle
    #[serde(default)]
    pub security: Option<SecurityConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema)]
pub struct NetworkConfig {
    pub public_ip: Option<bool>,
    pub enable_ipv6: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema)]
pub struct SecurityConfig {
    pub enable_firewall: Option<bool>,
}

fn default_boot_size() -> i32 {
    20
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
pub struct InstanceStatus {
    /// ID Scaleway de l'instance
    #[serde(default)]
    pub scaleway_id: Option<String>,

    /// IP publique
    #[serde(default)]
    pub public_ip: Option<String>,

    /// État actuel (running, stopped, booted, starting, stopping, rebooting, etc.)
    #[serde(default)]
    pub state: String,

    /// ID du projet Scaleway (pour tracking)
    #[serde(default)]
    pub project_id: Option<String>,

    /// Timestamp de création
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,

    /// Message d'erreur si applicable
    #[serde(default)]
    pub error_message: Option<String>,

    /// État de la synchronisation (Synced, Syncing, Error)
    #[serde(default)]
    pub sync_state: String,
}

impl Default for InstanceStatus {
    fn default() -> Self {
        Self {
            scaleway_id: None,
            public_ip: None,
            state: "unknown".to_string(),
            project_id: None,
            created_at: None,
            error_message: None,
            sync_state: "Syncing".to_string(),
        }
    }
}

// ============== LoadBalancer Resource ==============

#[derive(CustomResource, Serialize, Deserialize, Debug, Clone, JsonSchema)]
#[kube(group = "scaleway.mathieubodin.io", version = "v1", kind = "LoadBalancer")]
#[kube(namespaced)]
#[kube(status = "LoadBalancerStatus")]
#[kube(printcolumn = r#"{"name":"Scaleway ID","type":"string","jsonPath":".status.scalewayId"}"#)]
#[kube(printcolumn = r#"{"name":"State","type":"string","jsonPath":".status.state"}"#)]
#[kube(printcolumn = r#"{"name":"VIP","type":"string","jsonPath":".status.vipAddress"}"#)]
pub struct LoadBalancerSpec {
    /// Nom du load balancer Scaleway
    pub name: String,

    /// Zone Scaleway (fr-par-1, nl-ams-1, etc.)
    pub zone: String,

    /// Type de load balancer (LB-S, LB-GP, etc.)
    pub lb_type: String,

    /// Description optionnelle
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
pub struct LoadBalancerStatus {
    /// ID Scaleway du load balancer
    #[serde(default)]
    pub scaleway_id: Option<String>,

    /// Adresse VIP (IP publique du LB)
    #[serde(default)]
    pub vip_address: Option<String>,

    /// État actuel (pending, ready, error, etc.)
    #[serde(default)]
    pub state: String,

    /// ID du projet Scaleway (pour tracking)
    #[serde(default)]
    pub project_id: Option<String>,

    /// Timestamp de création
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,

    /// Message d'erreur si applicable
    #[serde(default)]
    pub error_message: Option<String>,

    /// État de la synchronisation (Pending, Synced, Syncing, Error, TerminationBlocked, etc.)
    #[serde(default)]
    pub sync_state: String,
}

impl Default for LoadBalancerStatus {
    fn default() -> Self {
        Self {
            scaleway_id: None,
            vip_address: None,
            state: String::new(),
            project_id: None,
            created_at: None,
            error_message: None,
            sync_state: "Pending".to_string(),
        }
    }
}

// ============== NamespaceRole Resource (Cluster-wide) ==============

#[derive(CustomResource, Serialize, Deserialize, Debug, Clone, JsonSchema)]
#[kube(
    group = "scaleway.mathieubodin.io",
    version = "v1",
    kind = "NamespaceRole"
)]
#[kube(status = "NamespaceRoleStatus")]
#[kube(printcolumn = r#"{"name":"Namespace","type":"string","jsonPath":".spec.namespace"}"#)]
#[kube(printcolumn = r#"{"name":"Role","type":"string","jsonPath":".spec.scalewayRole"}"#)]
pub struct NamespaceRoleSpec {
    /// Nom du namespace Kubernetes
    pub namespace: String,

    /// Rôle Scaleway à assumer pour ce namespace
    /// Exemples: "Editor", "Admin", "Viewer", "SecurityResponsible", "OrganizationOwner", etc.
    pub scaleway_role: String,

    /// Description optionnelle
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema)]
pub struct NamespaceRoleStatus {
    /// Timestamp de création/mise à jour
    #[serde(default)]
    pub last_updated: Option<DateTime<Utc>>,

    /// État de validation (Validated, Invalid, etc.)
    #[serde(default)]
    pub validation_state: String,

    /// Message d'erreur si applicable
    #[serde(default)]
    pub error_message: Option<String>,
}

impl Default for NamespaceRoleStatus {
    fn default() -> Self {
        Self {
            last_updated: None,
            validation_state: "Validated".to_string(),
            error_message: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_balancer_status_default_values() {
        let status = LoadBalancerStatus::default();
        assert_eq!(status.scaleway_id, None);
        assert_eq!(status.vip_address, None);
        assert_eq!(status.state, "");
        assert_eq!(status.project_id, None);
        assert_eq!(status.created_at, None);
        assert_eq!(status.error_message, None);
        assert_eq!(status.sync_state, "Pending");
    }

    #[test]
    fn test_load_balancer_status_serde_round_trip() {
        let status = LoadBalancerStatus {
            scaleway_id: Some("lb-abc123".to_string()),
            vip_address: Some("1.2.3.4".to_string()),
            state: "ready".to_string(),
            project_id: Some("proj-xyz".to_string()),
            created_at: None,
            error_message: None,
            sync_state: "Synced".to_string(),
        };
        let json = serde_json::to_string(&status).unwrap();
        let decoded: LoadBalancerStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, decoded);
    }

    #[test]
    fn test_load_balancer_status_missing_optional_fields_deserializes() {
        let json = r#"{"state":"pending","sync_state":"Pending"}"#;
        let status: LoadBalancerStatus = serde_json::from_str(json).unwrap();
        assert_eq!(status.scaleway_id, None);
        assert_eq!(status.vip_address, None);
        assert_eq!(status.state, "pending");
        assert_eq!(status.sync_state, "Pending");
    }

    #[test]
    fn test_instance_status_default_values() {
        let status = InstanceStatus::default();
        assert_eq!(status.scaleway_id, None);
        assert_eq!(status.public_ip, None);
        assert_eq!(status.state, "unknown");
        assert_eq!(status.project_id, None);
        assert_eq!(status.created_at, None);
        assert_eq!(status.error_message, None);
        assert_eq!(status.sync_state, "Syncing");
    }

    #[test]
    fn test_namespace_role_status_default_last_updated_is_none() {
        // Régression: l'ancien code appelait Utc::now() dans Default::default(),
        // rendant les assertions d'égalité non-déterministes.
        let status = NamespaceRoleStatus::default();
        assert_eq!(status.last_updated, None);
        assert_eq!(status.validation_state, "Validated");
        assert_eq!(status.error_message, None);
    }

    #[test]
    fn test_namespace_role_status_default_is_deterministic() {
        let s1 = NamespaceRoleStatus::default();
        let s2 = NamespaceRoleStatus::default();
        assert_eq!(s1.last_updated, s2.last_updated);
        assert_eq!(s1.validation_state, s2.validation_state);
    }
}
