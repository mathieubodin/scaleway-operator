use crate::error::{OperatorError, Result};
use crate::resources::InstanceSpec;
use reqwest::Client as ReqwestClient;
use serde_json::{json, Value};
use std::time::Duration;

const SCALEWAY_API_URL: &str = "https://api.scaleway.com";

#[derive(Clone)]
pub struct ScalewayClient {
    http_client: ReqwestClient,
    token: String,
    base_url: String,
}

impl ScalewayClient {
    pub fn new(token: String) -> Self {
        Self::new_with_base_url(token, SCALEWAY_API_URL.to_string())
    }

    pub fn new_with_base_url(token: String, base_url: String) -> Self {
        Self {
            http_client: ReqwestClient::builder()
                .timeout(Duration::from_secs(30))
                .connect_timeout(Duration::from_secs(10))
                .build()
                .expect("failed to build HTTP client"),
            token,
            base_url,
        }
    }

    // ============== Instance Operations ==============

    pub async fn create_instance(&self, spec: &InstanceSpec, project_id: &str) -> Result<String> {
        let body = json!({
            "name": &spec.name,
            "image": &spec.image,
            "commercial_type": &spec.instance_type,
            "tags": &spec.tags,
            "project_id": project_id,
            "boot_volume": {
                "size": spec.boot_volume_size as i64 * 1_000_000_000i64,
            }
        });

        let url = format!("{}/instance/v1/zones/{}/servers", self.base_url, &spec.zone);

        let response = self
            .http_client
            .post(&url)
            .header("X-Auth-Token", &self.token)
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status_code = response.status();
            let error_text = response.text().await?;
            return Err(OperatorError::ScalewayError {
                status: status_code.to_string(),
                message: error_text,
            });
        }

        let data: Value = response.json().await?;
        Ok(data["server"]["id"]
            .as_str()
            .ok_or_else(|| OperatorError::Unknown("No instance ID in response".to_string()))?
            .to_string())
    }

    /// Cherche une instance par nom dans un projet/zone. Retourne l'ID si trouvée.
    /// Utilisé pour récupérer une instance orpheline quand le status n'a pas pu être écrit.
    pub async fn find_instance_by_name(
        &self,
        zone: &str,
        name: &str,
        project_id: &str,
    ) -> Result<Option<String>> {
        let base_url = format!("{}/instance/v1/zones/{}/servers", self.base_url, zone);

        let response = self
            .http_client
            .get(&base_url)
            .query(&[("project_id", project_id), ("name", name)])
            .header("X-Auth-Token", &self.token)
            .send()
            .await?;

        match response.status() {
            s if s.is_success() => {}
            reqwest::StatusCode::NOT_FOUND => return Ok(None),
            s => {
                let status_code = s;
                let error_text = response.text().await?;
                return Err(OperatorError::ScalewayError {
                    status: status_code.to_string(),
                    message: error_text,
                });
            }
        }

        let data: Value = response.json().await?;
        Ok(data["servers"]
            .as_array()
            .and_then(|servers| servers.first())
            .and_then(|s| s["id"].as_str())
            .map(|id| id.to_string()))
    }

    pub async fn get_instance(
        &self,
        zone: &str,
        instance_id: &str,
        project_id: &str,
    ) -> Result<InstanceInfo> {
        let url = format!(
            "{}/instance/v1/zones/{}/servers/{}?project_id={}",
            self.base_url, zone, instance_id, project_id
        );

        let response = self
            .http_client
            .get(&url)
            .header("X-Auth-Token", &self.token)
            .send()
            .await?;

        match response.status() {
            s if s.is_success() => {}
            reqwest::StatusCode::NOT_FOUND => {
                return Err(OperatorError::InstanceNotFound(instance_id.to_string()));
            }
            s => {
                let status_code = s;
                let error_text = response.text().await?;
                return Err(OperatorError::ScalewayError {
                    status: status_code.to_string(),
                    message: error_text,
                });
            }
        }

        let data: Value = response.json().await?;
        let id = data["server"]["id"]
            .as_str()
            .ok_or_else(|| OperatorError::Unknown("Missing server.id in response".to_string()))?
            .to_string();

        Ok(InstanceInfo {
            id,
            state: data["server"]["state"]
                .as_str()
                .unwrap_or("unknown")
                .to_string(),
            public_ip: data["server"]["public_ip"]["address"]
                .as_str()
                .map(|s| s.to_string()),
            created_at: data["server"]["creation_date"]
                .as_str()
                .map(|s| s.to_string()),
        })
    }

    pub async fn delete_instance(&self, zone: &str, instance_id: &str) -> Result<()> {
        let url = format!(
            "{}/instance/v1/zones/{}/servers/{}",
            self.base_url, zone, instance_id
        );

        let response = self
            .http_client
            .delete(&url)
            .header("X-Auth-Token", &self.token)
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(());
        }

        if !response.status().is_success() {
            let status_code = response.status();
            let error_text = response.text().await?;
            return Err(OperatorError::ScalewayError {
                status: status_code.to_string(),
                message: error_text,
            });
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub async fn reboot_instance(&self, zone: &str, instance_id: &str) -> Result<()> {
        let url = format!(
            "{}/instance/v1/zones/{}/servers/{}/action",
            self.base_url, zone, instance_id
        );

        let body = json!({
            "action": "reboot"
        });

        let response = self
            .http_client
            .post(&url)
            .header("X-Auth-Token", &self.token)
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status_code = response.status();
            let error_text = response.text().await?;
            return Err(OperatorError::ScalewayError {
                status: status_code.to_string(),
                message: error_text,
            });
        }

        Ok(())
    }

    // ============== Project Operations ==============

    pub async fn verify_project_access(&self, project_id: &str) -> Result<()> {
        let url = format!("{}/account/v3/projects/{}", self.base_url, project_id);

        let response = self
            .http_client
            .get(&url)
            .header("X-Auth-Token", &self.token)
            .send()
            .await?;

        match response.status() {
            s if s.is_success() => Ok(()),
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
                Err(OperatorError::ProjectAccessDenied(project_id.to_string()))
            }
            reqwest::StatusCode::NOT_FOUND => Err(OperatorError::ConfigError(format!(
                "Project '{}' not found",
                project_id
            ))),
            s => {
                let status_code = s;
                let error_text = response.text().await?;
                Err(OperatorError::ScalewayError {
                    status: status_code.to_string(),
                    message: error_text,
                })
            }
        }
    }

    #[allow(dead_code)]
    pub async fn create_project(
        &self,
        org_id: &str,
        name: &str,
        description: Option<&str>,
    ) -> Result<String> {
        let body = json!({
            "name": name,
            "organization_id": org_id,
            "description": description.unwrap_or(""),
        });

        let url = format!("{}/account/v3/projects", self.base_url);

        let response = self
            .http_client
            .post(&url)
            .header("X-Auth-Token", &self.token)
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status_code = response.status();
            let error_text = response.text().await?;
            return Err(OperatorError::ScalewayError {
                status: status_code.to_string(),
                message: error_text,
            });
        }

        let data: Value = response.json().await?;
        Ok(data["project"]["id"]
            .as_str()
            .ok_or_else(|| OperatorError::Unknown("No project ID in response".to_string()))?
            .to_string())
    }

    #[allow(dead_code)]
    pub async fn delete_project(&self, project_id: &str) -> Result<()> {
        let url = format!("{}/account/v3/projects/{}", self.base_url, project_id);

        let response = self
            .http_client
            .delete(&url)
            .header("X-Auth-Token", &self.token)
            .send()
            .await?;

        if !response.status().is_success() {
            let status_code = response.status();
            let error_text = response.text().await?;
            return Err(OperatorError::ScalewayError {
                status: status_code.to_string(),
                message: error_text,
            });
        }

        Ok(())
    }

    // ============== Validation ==============

    pub async fn validate_zone(&self, zone: &str) -> Result<()> {
        // Zones valides Scaleway
        let valid_zones = [
            "fr-par-1", "fr-par-2", "nl-ams-1", "pl-waw-1", "sg-sin-1", "it-mil-1",
        ];

        if valid_zones.contains(&zone) {
            Ok(())
        } else {
            Err(OperatorError::InvalidZone(zone.to_string()))
        }
    }

    pub async fn validate_instance_type(&self, instance_type: &str) -> Result<()> {
        // Types valides (non exhaustif)
        let valid_types = [
            "DEV1-S", "DEV1-M", "DEV1-L", "DEV1-XL", "GP1-XS", "GP1-S", "GP1-M", "GP1-L", "GP1-XL",
            "CPU1-XS", "CPU1-S", "CPU1-M", "CPU1-L", "GPU-3090", "GPU-4090",
        ];

        if valid_types.contains(&instance_type) {
            Ok(())
        } else {
            Err(OperatorError::InvalidInstanceType(
                instance_type.to_string(),
            ))
        }
    }
}

#[derive(Debug, Clone)]
pub struct InstanceInfo {
    #[allow(dead_code)]
    pub id: String,
    pub state: String,
    pub public_ip: Option<String>,
    #[allow(dead_code)]
    pub created_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::InstanceSpec;

    fn test_client() -> ScalewayClient {
        ScalewayClient::new("test-token".to_string())
    }

    fn test_spec() -> InstanceSpec {
        InstanceSpec {
            name: "my-instance".to_string(),
            zone: "fr-par-1".to_string(),
            image: "ubuntu-jammy".to_string(),
            instance_type: "DEV1-S".to_string(),
            tags: vec![],
            boot_volume_size: 20,
            network: None,
            security: None,
        }
    }

    // --- validate_zone ---

    #[tokio::test]
    async fn test_validate_zone_valid_fr_par_1() {
        assert!(test_client().validate_zone("fr-par-1").await.is_ok());
    }

    #[tokio::test]
    async fn test_validate_zone_valid_it_mil_1() {
        assert!(test_client().validate_zone("it-mil-1").await.is_ok());
    }

    #[tokio::test]
    async fn test_validate_zone_all_valid() {
        let client = test_client();
        for zone in &[
            "fr-par-1", "fr-par-2", "nl-ams-1", "pl-waw-1", "sg-sin-1", "it-mil-1",
        ] {
            assert!(
                client.validate_zone(zone).await.is_ok(),
                "Zone {} should be valid",
                zone
            );
        }
    }

    #[tokio::test]
    async fn test_validate_zone_invalid_us_east() {
        let result = test_client().validate_zone("us-east-1").await;
        assert!(
            matches!(result, Err(crate::error::OperatorError::InvalidZone(z)) if z == "us-east-1")
        );
    }

    #[tokio::test]
    async fn test_validate_zone_empty_string() {
        let result = test_client().validate_zone("").await;
        assert!(matches!(
            result,
            Err(crate::error::OperatorError::InvalidZone(_))
        ));
    }

    // --- validate_instance_type ---

    #[tokio::test]
    async fn test_validate_instance_type_dev1_s() {
        assert!(test_client().validate_instance_type("DEV1-S").await.is_ok());
    }

    #[tokio::test]
    async fn test_validate_instance_type_gp1_xl() {
        assert!(test_client().validate_instance_type("GP1-XL").await.is_ok());
    }

    #[tokio::test]
    async fn test_validate_instance_type_unknown() {
        let result = test_client().validate_instance_type("MEGA-XL").await;
        assert!(
            matches!(result, Err(crate::error::OperatorError::InvalidInstanceType(t)) if t == "MEGA-XL")
        );
    }

    #[tokio::test]
    async fn test_validate_instance_type_case_sensitive() {
        // Les types sont en majuscules — "dev1-s" doit échouer
        let result = test_client().validate_instance_type("dev1-s").await;
        assert!(matches!(
            result,
            Err(crate::error::OperatorError::InvalidInstanceType(_))
        ));
    }

    // --- HTTP tests avec mockito ---

    #[tokio::test]
    async fn test_create_instance_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/instance/v1/zones/fr-par-1/servers")
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(r#"{"server": {"id": "srv-abc123"}}"#)
            .create_async()
            .await;

        let client = ScalewayClient::new_with_base_url("tok".into(), server.url());
        let result = client.create_instance(&test_spec(), "proj-x").await;

        assert!(result.is_ok(), "Expected Ok, got {:?}", result);
        assert_eq!(result.unwrap(), "srv-abc123");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_create_instance_error_returns_scaleway_error() {
        // Régression use-after-move: vérifie que status ET body sont capturés
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/instance/v1/zones/fr-par-1/servers")
            .with_status(422)
            .with_header("content-type", "application/json")
            .with_body(r#"{"message": "invalid spec"}"#)
            .create_async()
            .await;

        let client = ScalewayClient::new_with_base_url("tok".into(), server.url());
        let result = client.create_instance(&test_spec(), "proj-x").await;

        assert!(result.is_err());
        if let Err(crate::error::OperatorError::ScalewayError { status, message }) = result {
            assert!(
                status.contains("422"),
                "Expected 422 in status, got: {}",
                status
            );
            assert!(message.contains("invalid spec"));
        } else {
            panic!("Expected ScalewayError");
        }
    }

    #[tokio::test]
    async fn test_find_instance_by_name_found() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock(
                "GET",
                mockito::Matcher::Regex(r"/instance/v1/zones/fr-par-1/servers".to_string()),
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"servers": [{"id": "srv-found"}]}"#)
            .create_async()
            .await;

        let client = ScalewayClient::new_with_base_url("tok".into(), server.url());
        let result = client
            .find_instance_by_name("fr-par-1", "my-instance", "proj-x")
            .await;

        assert_eq!(result.unwrap(), Some("srv-found".to_string()));
    }

    #[tokio::test]
    async fn test_find_instance_by_name_not_found_empty_list() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock(
                "GET",
                mockito::Matcher::Regex(r"/instance/v1/zones/fr-par-1/servers".to_string()),
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"servers": []}"#)
            .create_async()
            .await;

        let client = ScalewayClient::new_with_base_url("tok".into(), server.url());
        let result = client
            .find_instance_by_name("fr-par-1", "ghost", "proj-x")
            .await;

        assert_eq!(result.unwrap(), None);
    }

    #[tokio::test]
    async fn test_find_instance_by_name_404_returns_none() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock(
                "GET",
                mockito::Matcher::Regex(r"/instance/v1/zones/fr-par-1/servers".to_string()),
            )
            .with_status(404)
            .with_body("")
            .create_async()
            .await;

        let client = ScalewayClient::new_with_base_url("tok".into(), server.url());
        let result = client
            .find_instance_by_name("fr-par-1", "x", "proj-x")
            .await;

        assert_eq!(result.unwrap(), None);
    }

    #[tokio::test]
    async fn test_find_instance_by_name_403_returns_err() {
        // Régression: 403 ne doit PAS retourner Ok(None) — c'était le bug de création de doublons
        let mut server = mockito::Server::new_async().await;
        server
            .mock(
                "GET",
                mockito::Matcher::Regex(r"/instance/v1/zones/fr-par-1/servers".to_string()),
            )
            .with_status(403)
            .with_body(r#"{"message": "forbidden"}"#)
            .create_async()
            .await;

        let client = ScalewayClient::new_with_base_url("tok".into(), server.url());
        let result = client
            .find_instance_by_name("fr-par-1", "x", "proj-x")
            .await;

        assert!(result.is_err(), "403 should return Err, not Ok(None)");
        assert!(matches!(
            result.unwrap_err(),
            crate::error::OperatorError::ScalewayError { .. }
        ));
    }

    #[tokio::test]
    async fn test_find_instance_by_name_429_returns_err() {
        // Régression: 429 rate-limit ne doit PAS retourner Ok(None)
        let mut server = mockito::Server::new_async().await;
        server
            .mock(
                "GET",
                mockito::Matcher::Regex(r"/instance/v1/zones/fr-par-1/servers".to_string()),
            )
            .with_status(429)
            .with_body(r#"{"message": "rate limited"}"#)
            .create_async()
            .await;

        let client = ScalewayClient::new_with_base_url("tok".into(), server.url());
        let result = client
            .find_instance_by_name("fr-par-1", "x", "proj-x")
            .await;

        assert!(result.is_err(), "429 should return Err, not Ok(None)");
    }

    #[tokio::test]
    async fn test_get_instance_success() {
        let mut server = mockito::Server::new_async().await;
        server.mock("GET", mockito::Matcher::Regex(r"/instance/v1/zones/fr-par-1/servers/srv-123".to_string()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"server": {"id": "srv-123", "state": "running", "public_ip": {"address": "1.2.3.4"}, "creation_date": "2026-01-01T00:00:00Z"}}"#)
            .create_async()
            .await;

        let client = ScalewayClient::new_with_base_url("tok".into(), server.url());
        let result = client.get_instance("fr-par-1", "srv-123", "proj-x").await;

        assert!(result.is_ok());
        let info = result.unwrap();
        assert_eq!(info.id, "srv-123");
        assert_eq!(info.state, "running");
        assert_eq!(info.public_ip, Some("1.2.3.4".to_string()));
    }

    #[tokio::test]
    async fn test_get_instance_404_returns_not_found() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock(
                "GET",
                mockito::Matcher::Regex(
                    r"/instance/v1/zones/fr-par-1/servers/srv-gone".to_string(),
                ),
            )
            .with_status(404)
            .with_body("")
            .create_async()
            .await;

        let client = ScalewayClient::new_with_base_url("tok".into(), server.url());
        let result = client.get_instance("fr-par-1", "srv-gone", "proj-x").await;

        assert!(
            matches!(result, Err(crate::error::OperatorError::InstanceNotFound(id)) if id == "srv-gone")
        );
    }

    #[tokio::test]
    async fn test_get_instance_401_returns_scaleway_error_not_not_found() {
        // 401 ne doit PAS être mapé sur InstanceNotFound
        let mut server = mockito::Server::new_async().await;
        server
            .mock(
                "GET",
                mockito::Matcher::Regex(r"/instance/v1/zones/fr-par-1/servers/srv-x".to_string()),
            )
            .with_status(401)
            .with_body(r#"{"message": "unauthorized"}"#)
            .create_async()
            .await;

        let client = ScalewayClient::new_with_base_url("tok".into(), server.url());
        let result = client.get_instance("fr-par-1", "srv-x", "proj-x").await;

        assert!(matches!(
            result,
            Err(crate::error::OperatorError::ScalewayError { .. })
        ));
    }

    #[tokio::test]
    async fn test_delete_instance_success() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("DELETE", "/instance/v1/zones/fr-par-1/servers/srv-del")
            .with_status(204)
            .with_body("")
            .create_async()
            .await;

        let client = ScalewayClient::new_with_base_url("tok".into(), server.url());
        assert!(client.delete_instance("fr-par-1", "srv-del").await.is_ok());
    }

    #[tokio::test]
    async fn test_delete_instance_404_is_success() {
        // Idempotence : instance déjà supprimée → Ok(())
        let mut server = mockito::Server::new_async().await;
        server
            .mock("DELETE", "/instance/v1/zones/fr-par-1/servers/srv-gone")
            .with_status(404)
            .with_body("")
            .create_async()
            .await;

        let client = ScalewayClient::new_with_base_url("tok".into(), server.url());
        assert!(client.delete_instance("fr-par-1", "srv-gone").await.is_ok());
    }

    #[tokio::test]
    async fn test_verify_project_access_success() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/account/v3/projects/proj-abc")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"id": "proj-abc"}"#)
            .create_async()
            .await;

        let client = ScalewayClient::new_with_base_url("tok".into(), server.url());
        assert!(client.verify_project_access("proj-abc").await.is_ok());
    }

    #[tokio::test]
    async fn test_verify_project_access_403_is_permanent_error() {
        // 403 → ProjectAccessDenied (permanent, await_change dans error_policy)
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/account/v3/projects/proj-x")
            .with_status(403)
            .with_body("")
            .create_async()
            .await;

        let client = ScalewayClient::new_with_base_url("tok".into(), server.url());
        let result = client.verify_project_access("proj-x").await;

        assert!(matches!(
            result,
            Err(crate::error::OperatorError::ProjectAccessDenied(_))
        ));
    }

    #[tokio::test]
    async fn test_verify_project_access_404_is_config_error() {
        // 404 → ConfigError (permanent) — projet inexistant
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/account/v3/projects/proj-missing")
            .with_status(404)
            .with_body("")
            .create_async()
            .await;

        let client = ScalewayClient::new_with_base_url("tok".into(), server.url());
        let result = client.verify_project_access("proj-missing").await;

        assert!(matches!(
            result,
            Err(crate::error::OperatorError::ConfigError(_))
        ));
    }

    #[tokio::test]
    async fn test_verify_project_access_500_is_transient_error() {
        // Régression: 500 ne doit PAS mapper sur ProjectAccessDenied (permanent).
        // ScalewayError → requeue 60s dans error_policy.
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/account/v3/projects/proj-x")
            .with_status(500)
            .with_body(r#"{"message": "internal error"}"#)
            .create_async()
            .await;

        let client = ScalewayClient::new_with_base_url("tok".into(), server.url());
        let result = client.verify_project_access("proj-x").await;

        assert!(
            matches!(
                result,
                Err(crate::error::OperatorError::ScalewayError { .. })
            ),
            "500 doit être un ScalewayError (transitoire), pas ProjectAccessDenied (permanent)"
        );
    }
}
