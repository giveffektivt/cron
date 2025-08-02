use anyhow::{Context, Result};
use reqwest::Client;

pub fn parse_env(key: &str) -> Result<String> {
    std::env::var(key).with_context(|| format!("environment variable '{key}' not set"))
}

pub async fn healthy(http: &Client, url: &str, name: &str) {
    log::trace!("Reporting health");

    match http.post(url).send().await {
        Ok(h) => {
            let status = h.status();
            let response = h.text().await.unwrap_or("<no response>".to_string());
            if !status.is_success() {
                log::error!("Error reporting [{name}] health: {status} {response}");
            }
        }
        Err(e) => log::error!("Error reporting [{name}] health: {e}"),
    }
}
