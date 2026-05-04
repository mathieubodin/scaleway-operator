use chrono::{DateTime, Utc};
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ============== Instance Resource ==============

#[derive(CustomResource, Serialize, Deserialize, Debug, Clone, JsonSchema)]
#[kube(group = "scaleway.io", version = "v1", kind = "Instance")]
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

// ============== Project Resource ==============

#[allow(dead_code)]
#[derive(CustomResource, Serialize, Deserialize, Debug, Clone, JsonSchema)]
#[kube(group = "scaleway.io", version = "v1", kind = "Project")]
#[kube(namespaced)]
#[kube(status = "ProjectStatus")]
pub struct ProjectSpec {
    /// Nom du projet Scaleway
    pub name: String,

    /// ID de l'organisation
    pub organization_id: String,

    /// Description optionnelle
    #[serde(default)]
    pub description: Option<String>,
}

#[allow(dead_code)]
#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema)]
pub struct ProjectStatus {
    /// ID Scaleway du projet
    #[serde(default)]
    pub scaleway_project_id: Option<String>,

    /// Timestamp de création
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,

    /// État de synchronisation
    #[serde(default)]
    pub sync_state: String,
}

impl Default for ProjectStatus {
    fn default() -> Self {
        Self {
            scaleway_project_id: None,
            created_at: None,
            sync_state: "Syncing".to_string(),
        }
    }
}

// ============== LoadBalancer Resource ==============

#[allow(dead_code)]
#[derive(CustomResource, Serialize, Deserialize, Debug, Clone, JsonSchema)]
#[kube(group = "scaleway.io", version = "v1", kind = "LoadBalancer")]
#[kube(namespaced)]
#[kube(status = "LoadBalancerStatus")]
pub struct LoadBalancerSpec {
    /// Nom du Load Balancer
    pub name: String,

    /// Zone Scaleway
    pub zone: String,

    /// ID du projet
    pub project_id: String,

    /// Type (lb-s, lb-gp-m, etc.)
    pub lb_type: String,

    /// Configuration backend
    pub backends: Vec<BackendConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema)]
pub struct BackendConfig {
    pub name: String,
    pub protocol: String, // http, https, tcp
    pub port: i32,
}

#[allow(dead_code)]
#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema)]
pub struct LoadBalancerStatus {
    #[serde(default)]
    pub scaleway_id: Option<String>,

    #[serde(default)]
    pub ip_address: Option<String>,

    #[serde(default)]
    pub state: String,
}

impl Default for LoadBalancerStatus {
    fn default() -> Self {
        Self {
            scaleway_id: None,
            ip_address: None,
            state: "unknown".to_string(),
        }
    }
}

// ============== NamespaceRole Resource (Cluster-wide) ==============

#[derive(CustomResource, Serialize, Deserialize, Debug, Clone, JsonSchema)]
#[kube(group = "scaleway.io", version = "v1", kind = "NamespaceRole")]
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
