use anyhow::{Result, anyhow};
use chrono::{Duration, Utc};
use reqwest::Client;
use serde_json::Value;
use sqlx::{PgPool, query, types::BigDecimal};
use std::env;
use tokio::time::{Duration as Td, interval};

pub async fn clearhaus(http: Client, db: PgPool) -> Result<()> {
    let base_url = std::env::var("CLEARHAUS_BASE_URL")?;
    let health_url = env::var("CLEARHAUS_HEALTHCHECK_URL")?;
    let client_id = env::var("CLEARHAUS_CLIENT_ID")?;
    let client_secret = env::var("CLEARHAUS_CLIENT_SECRET")?;
    let interval_secs = env::var("CLEARHAUS_INTERVAL_SECONDS")?.parse()?;

    if interval_secs < 1 {
        log::warn!("Task disabled, exiting");
        return Ok(());
    }

    let mut token = String::new();
    let mut expired_at = Utc::now();

    let mut ticker = interval(Td::from_secs(interval_secs));
    loop {
        ticker.tick().await;
        log::info!("Starting");

        if Utc::now() >= expired_at - Duration::minutes(15) {
            log::info!("Refreshing auth token");
            let r = http
                .post(format!("{base_url}/oauth/token"))
                .basic_auth(&client_id, Some(&client_secret))
                .form(&[
                    ("grant_type", "client_credentials"),
                    ("audience", &base_url),
                ])
                .send()
                .await?;

            let j = r.json::<Value>().await?;

            token = j["access_token"]
                .as_str()
                .ok_or(anyhow!("Error parsing token from {j}"))?
                .to_string();

            expired_at = Utc::now()
                + Duration::seconds(
                    j["expires_in"]
                        .as_i64()
                        .ok_or(anyhow!("Error parsing token expiration from {j}"))?,
                );
        }

        log::trace!("Fetching settlements");
        let r = http
            .get(format!("{base_url}/settlements?query=settled%3Afalse"))
            .bearer_auth(&token)
            .send()
            .await?;

        if !r.status().is_success() {
            log::error!(
                "Error fetching settlements: {} {}",
                r.status(),
                r.text().await?
            );
            continue;
        }

        log::trace!("Processing response");
        let j = r.json::<Value>().await?;
        for s in j["_embedded"]["ch:settlements"]
            .as_array()
            .ok_or(anyhow!("Error parsing settlements array from {j}"))?
        {
            let merchant_id = s["_embedded"]["ch:account"]["merchant_id"]
                .as_str()
                .ok_or(anyhow!("Error parsing merchant ID from {s}"))?
                .parse::<i32>()?;

            let amount = s["summary"]["net"]
                .as_i64()
                .ok_or(anyhow!("Error parsing net amount from {s}"))?;

            match query!(
                "INSERT INTO clearhaus_settlement(merchant_id, amount) VALUES($1, $2)",
                BigDecimal::from(merchant_id),
                BigDecimal::new(amount.into(), 2)
            )
            .execute(&db)
            .await
            {
                Ok(_) => log::debug!("OK merchant_id={merchant_id} amount={amount}"),
                Err(err) => log::error!(
                    "Error saving into DB merchant_id={merchant_id} value={amount}: {err}"
                ),
            }
        }

        log::trace!("Reporting health");
        let h = http.post(&health_url).send().await?;
        if !h.status().is_success() {
            log::error!("Error reporting health: {} {}", h.status(), h.text().await?);
        }

        log::info!("Success");
    }
}
