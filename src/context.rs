use crate::scaleway::ScalewayClient;
use kube::Client;

const SCALEWAY_PROJECT_ANNOTATION: &str = "scaleway.mathieubodin.io/project-id";

pub struct Context {
    pub client: Client,
    pub scaleway_client: ScalewayClient,
    pub organization_id: String,
    pub scaleway_base_url: String,
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
