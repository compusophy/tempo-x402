//! Railway GraphQL API client.
//!
//! Wraps the Railway platform API (`https://backboard.railway.app/graphql/v2`)
//! for programmatic service creation, environment variable configuration,
//! Docker image deployment, and domain management.

use serde::{Deserialize, Serialize};
use std::time::Duration;

const RAILWAY_API_URL: &str = "https://backboard.railway.app/graphql/v2";
const MAX_RETRIES: u32 = 5;
const BASE_DELAY_MS: u64 = 1000;

#[derive(Debug, thiserror::Error)]
pub enum RailwayError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("GraphQL error: {0}")]
    GraphQL(String),

    #[error("missing field in response: {0}")]
    MissingField(String),

    #[error("HTTP {status}: {body}")]
    HttpStatus { status: u16, body: String },

    #[error("exhausted {attempts} retries, last error: {source}")]
    Exhausted {
        attempts: u32,
        source: Box<RailwayError>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct GraphQLRequest {
    query: String,
    variables: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct GraphQLResponse {
    data: Option<serde_json::Value>,
    errors: Option<Vec<GraphQLError>>,
}

#[derive(Debug, Deserialize)]
struct GraphQLError {
    message: String,
}

/// Client for the Railway platform GraphQL API.
pub struct RailwayClient {
    http: reqwest::Client,
    token: String,
    project_id: String,
}

impl RailwayClient {
    pub fn new(token: String, project_id: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("failed to create HTTP client");

        Self {
            http,
            token,
            project_id,
        }
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    /// Returns true if the HTTP status code is retryable (server error or rate limit).
    fn is_retryable_status(status: reqwest::StatusCode) -> bool {
        matches!(status.as_u16(), 429 | 502 | 503 | 504)
    }

    /// Returns true if a reqwest error is retryable (timeout or connection).
    fn is_retryable_error(err: &reqwest::Error) -> bool {
        err.is_timeout() || err.is_connect() || err.is_request()
    }

    /// Compute delay for a retry attempt with ±25% jitter.
    fn retry_delay(attempt: u32, retry_after: Option<Duration>) -> Duration {
        if let Some(ra) = retry_after {
            return ra;
        }
        // Exponential: 500ms, 1000ms, 2000ms
        let base_ms = BASE_DELAY_MS * 2u64.pow(attempt);
        // Jitter: ±25%
        let jitter_range = base_ms / 4;
        let jitter = (base_ms as i64)
            + (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos() as i64
                % (2 * jitter_range as i64 + 1))
            - jitter_range as i64;
        Duration::from_millis(jitter.max(100) as u64)
    }

    /// Parse the `Retry-After` header value (seconds) from a response.
    fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
        headers
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .map(Duration::from_secs)
    }

    /// Execute a GraphQL query/mutation against the Railway API with retry.
    async fn execute(
        &self,
        query: &str,
        variables: serde_json::Value,
    ) -> Result<serde_json::Value, RailwayError> {
        let request = GraphQLRequest {
            query: query.to_string(),
            variables,
        };

        let mut last_err: Option<RailwayError> = None;

        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                let delay = Self::retry_delay(attempt - 1, None);
                tracing::warn!(
                    attempt,
                    delay_ms = delay.as_millis() as u64,
                    "Retrying Railway API request"
                );
                tokio::time::sleep(delay).await;
            }

            // Send request
            let response = match self
                .http
                .post(RAILWAY_API_URL)
                .header("Authorization", format!("Bearer {}", self.token))
                .header("Content-Type", "application/json")
                .json(&request)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    if Self::is_retryable_error(&e) && attempt < MAX_RETRIES {
                        tracing::warn!(attempt, error = %e, "Retryable request error");
                        last_err = Some(RailwayError::Http(e));
                        continue;
                    }
                    return Err(RailwayError::Http(e));
                }
            };

            // Check HTTP status before parsing body
            let status = response.status();
            if !status.is_success() {
                if Self::is_retryable_status(status) && attempt < MAX_RETRIES {
                    let retry_after = Self::parse_retry_after(response.headers());
                    let body = response.text().await.unwrap_or_default();
                    tracing::warn!(attempt, status = status.as_u16(), "Retryable HTTP status");
                    if let Some(ra) = retry_after {
                        // Override delay with Retry-After for the next attempt
                        tokio::time::sleep(ra).await;
                        last_err = Some(RailwayError::HttpStatus {
                            status: status.as_u16(),
                            body,
                        });
                        // Skip the normal delay at the top of the loop — we already slept
                        // We do this by just continuing (next iteration's delay uses attempt)
                        continue;
                    }
                    last_err = Some(RailwayError::HttpStatus {
                        status: status.as_u16(),
                        body,
                    });
                    continue;
                }
                let body = response.text().await.unwrap_or_default();
                return Err(RailwayError::HttpStatus {
                    status: status.as_u16(),
                    body,
                });
            }

            // Parse response body
            let gql_response: GraphQLResponse = match response.json().await {
                Ok(r) => r,
                Err(e) => {
                    if attempt < MAX_RETRIES {
                        tracing::warn!(attempt, error = %e, "Failed to parse response JSON");
                        last_err = Some(RailwayError::Http(e));
                        continue;
                    }
                    return Err(RailwayError::Http(e));
                }
            };

            if let Some(errors) = gql_response.errors {
                let messages: Vec<String> = errors.into_iter().map(|e| e.message).collect();
                return Err(RailwayError::GraphQL(messages.join("; ")));
            }

            return gql_response
                .data
                .ok_or_else(|| RailwayError::MissingField("data".to_string()));
        }

        // All retries exhausted
        Err(RailwayError::Exhausted {
            attempts: MAX_RETRIES + 1,
            source: Box::new(last_err.unwrap_or_else(|| {
                RailwayError::MissingField("unknown error after retries".to_string())
            })),
        })
    }

    /// Create a new service in the project.
    pub async fn create_service(&self, name: &str) -> Result<String, RailwayError> {
        let query = r#"
            mutation ServiceCreate($input: ServiceCreateInput!) {
                serviceCreate(input: $input) {
                    id
                    name
                }
            }
        "#;

        let variables = serde_json::json!({
            "input": {
                "projectId": self.project_id,
                "name": name,
            }
        });

        let data = self.execute(query, variables).await?;
        data["serviceCreate"]["id"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| RailwayError::MissingField("serviceCreate.id".to_string()))
    }

    /// Get the default environment ID for the project.
    pub async fn get_default_environment(&self) -> Result<String, RailwayError> {
        let query = r#"
            query Project($id: String!) {
                project(id: $id) {
                    environments {
                        edges {
                            node {
                                id
                                name
                            }
                        }
                    }
                }
            }
        "#;

        let variables = serde_json::json!({ "id": self.project_id });
        let data = self.execute(query, variables).await?;

        // Return the first environment (Railway projects have a default "production" env)
        data["project"]["environments"]["edges"]
            .as_array()
            .and_then(|edges| edges.first())
            .and_then(|edge| edge["node"]["id"].as_str())
            .map(String::from)
            .ok_or_else(|| RailwayError::MissingField("environment id".to_string()))
    }

    /// Set environment variables on a service.
    pub async fn set_variables(
        &self,
        service_id: &str,
        environment_id: &str,
        variables: serde_json::Value,
    ) -> Result<(), RailwayError> {
        let query = r#"
            mutation VariableCollectionUpsert($input: VariableCollectionUpsertInput!) {
                variableCollectionUpsert(input: $input)
            }
        "#;

        let input = serde_json::json!({
            "input": {
                "projectId": self.project_id,
                "serviceId": service_id,
                "environmentId": environment_id,
                "variables": variables,
            }
        });

        self.execute(query, input).await?;
        Ok(())
    }

    /// Set the Docker image source for a service.
    pub async fn set_docker_image(
        &self,
        service_id: &str,
        image: &str,
    ) -> Result<(), RailwayError> {
        let query = r#"
            mutation ServiceInstanceUpdate($serviceId: String!, $input: ServiceInstanceUpdateInput!) {
                serviceInstanceUpdate(serviceId: $serviceId, input: $input)
            }
        "#;

        let variables = serde_json::json!({
            "serviceId": service_id,
            "input": {
                "source": {
                    "image": image,
                }
            }
        });

        self.execute(query, variables).await?;
        Ok(())
    }

    /// Create a public domain for a service in an environment.
    pub async fn create_domain(
        &self,
        service_id: &str,
        environment_id: &str,
    ) -> Result<String, RailwayError> {
        let query = r#"
            mutation ServiceDomainCreate($input: ServiceDomainCreateInput!) {
                serviceDomainCreate(input: $input) {
                    id
                    domain
                }
            }
        "#;

        let variables = serde_json::json!({
            "input": {
                "serviceId": service_id,
                "environmentId": environment_id,
            }
        });

        let data = self.execute(query, variables).await?;
        data["serviceDomainCreate"]["domain"]
            .as_str()
            .map(|d| format!("https://{d}"))
            .ok_or_else(|| RailwayError::MissingField("serviceDomainCreate.domain".to_string()))
    }

    /// Add a persistent volume to a service.
    pub async fn add_volume(
        &self,
        service_id: &str,
        environment_id: &str,
        mount_path: &str,
    ) -> Result<String, RailwayError> {
        let query = r#"
            mutation VolumeCreate($input: VolumeCreateInput!) {
                volumeCreate(input: $input) {
                    id
                }
            }
        "#;

        let variables = serde_json::json!({
            "input": {
                "projectId": self.project_id,
                "serviceId": service_id,
                "environmentId": environment_id,
                "mountPath": mount_path,
            }
        });

        let data = self.execute(query, variables).await?;
        data["volumeCreate"]["id"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| RailwayError::MissingField("volumeCreate.id".to_string()))
    }

    /// Delete a Railway service. Best-effort cleanup — logs on failure.
    pub async fn delete_service(&self, service_id: &str) -> Result<(), RailwayError> {
        let query = r#"
            mutation ServiceDelete($id: String!) {
                serviceDelete(id: $id)
            }
        "#;

        let variables = serde_json::json!({ "id": service_id });
        self.execute(query, variables).await?;
        Ok(())
    }

    /// Update compute resource limits for a service instance.
    ///
    /// Uses `serviceInstanceLimitsUpdate` mutation.
    /// Railway expects CPU as vCPUs (Float) and memory as GB (Float).
    pub async fn update_service_resources(
        &self,
        service_id: &str,
        environment_id: &str,
        cpu_limit_millicores: u32,
        memory_limit_mb: u32,
    ) -> Result<(), RailwayError> {
        let query = r#"
            mutation ServiceInstanceLimitsUpdate($input: ServiceInstanceLimitsUpdateInput!) {
                serviceInstanceLimitsUpdate(input: $input)
            }
        "#;

        let vcpus = cpu_limit_millicores as f64 / 1000.0;
        let memory_gb = memory_limit_mb as f64 / 1024.0;

        let variables = serde_json::json!({
            "input": {
                "serviceId": service_id,
                "environmentId": environment_id,
                "vCPUs": vcpus,
                "memoryGB": memory_gb,
            }
        });

        self.execute(query, variables).await?;
        Ok(())
    }

    /// Connect a Railway service to a GitHub repo + branch (source-based builds).
    ///
    /// Uses `serviceInstanceUpdate` with `source: { repo, branch }` to switch
    /// a service from Docker image–based to source-based builds.
    pub async fn connect_repo(
        &self,
        service_id: &str,
        environment_id: &str,
        repo: &str,
        branch: &str,
    ) -> Result<(), RailwayError> {
        // 1. Set the source repo (ServiceSourceInput only accepts repo + image, not branch)
        let source_query = r#"
            mutation ServiceInstanceUpdate($serviceId: String!, $input: ServiceInstanceUpdateInput!) {
                serviceInstanceUpdate(serviceId: $serviceId, input: $input)
            }
        "#;

        let source_vars = serde_json::json!({
            "serviceId": service_id,
            "input": {
                "source": {
                    "repo": repo,
                }
            }
        });

        self.execute(source_query, source_vars).await?;

        // 2. Create a deployment trigger to set the branch
        let trigger_query = r#"
            mutation DeploymentTriggerCreate($input: DeploymentTriggerCreateInput!) {
                deploymentTriggerCreate(input: $input) { id }
            }
        "#;

        let trigger_vars = serde_json::json!({
            "input": {
                "serviceId": service_id,
                "projectId": self.project_id,
                "environmentId": environment_id,
                "provider": "github",
                "repository": repo,
                "branch": branch,
            }
        });

        self.execute(trigger_query, trigger_vars).await?;
        Ok(())
    }

    /// Trigger a deployment for a service in an environment.
    ///
    /// Uses `serviceInstanceDeploy` which works for Docker image–based services.
    /// (`environmentTriggersDeploy` only triggers git-based deploys.)
    pub async fn deploy_service(
        &self,
        service_id: &str,
        environment_id: &str,
    ) -> Result<String, RailwayError> {
        let query = r#"
            mutation ServiceInstanceDeploy($serviceId: String!, $environmentId: String!) {
                serviceInstanceDeploy(serviceId: $serviceId, environmentId: $environmentId)
            }
        "#;

        let variables = serde_json::json!({
            "serviceId": service_id,
            "environmentId": environment_id,
        });

        self.execute(query, variables).await?;
        Ok("triggered".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_railway_client_creation() {
        let client = RailwayClient::new("test-token".to_string(), "test-project".to_string());
        assert_eq!(client.project_id(), "test-project");
    }
}
