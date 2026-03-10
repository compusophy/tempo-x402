use serde::{Serialize, Deserialize};
use reqwest::Client;
use std::time::Duration;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HealthStatus {
    pub status: String,
    pub uptime: u64,
    pub version: String,
}

pub async fn check_peer_health(url: &str) -> Result<HealthStatus, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client.get(format!("{}/health", url))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if resp.status().is_success() {
        let status = resp.json::<HealthStatus>()
            .await
            .map_err(|e| e.to_string())?;
        Ok(status)
    } else {
        Err(format!("Peer returned status: {}", resp.status()))
    }
}

pub async fn check_multiple_peers(urls: Vec<String>) -> Vec<(String, Result<HealthStatus, String>)> {
    let mut tasks = Vec::new();
    for url in urls {
        tasks.push(tokio::spawn(async move {
            let res = check_peer_health(&url).await;
            (url, res)
        }));
    }

    let mut results = Vec::new();
    for task in tasks {
        if let Ok(res) = task.await {
            results.push(res);
        }
    }
    results
}
