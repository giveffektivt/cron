use crate::{
    cron::{CronJob, spawn_cron},
    helpers::{healthy, parse_env},
};
use anyhow::{Result, anyhow};
use chrono::{DateTime, Duration as Cd, Utc};
use futures::{FutureExt, future::BoxFuture};
use reqwest::Client;
use serde_json::Value;
use sqlx::{PgPool, query, types::BigDecimal};
use std::time::Duration;

pub fn start(http: Client, db: PgPool) {
    match parse_env("CLEARHAUS_INTERVAL_SECONDS").and_then(|v| v.parse().map_err(Into::into)) {
        Ok(interval_secs) => {
            spawn_cron(
                "clearhaus",
                Duration::from_secs(interval_secs),
                Task::new(http, db).expect("Invalid config"),
            );
        }
        Err(_) => log::warn!("[clearhaus] disabled (no/invalid interval)"),
    };
}

struct Task {
    base_url: String,
    client_id: String,
    client_secret: String,
    token: String,
    expires_at: DateTime<Utc>,
    health_url: String,
    http: Client,
    db: PgPool,
}

impl Task {
    fn new(http: Client, db: PgPool) -> Result<Self> {
        Ok(Self {
            base_url: parse_env("CLEARHAUS_BASE_URL")?,
            client_id: parse_env("CLEARHAUS_CLIENT_ID")?,
            client_secret: parse_env("CLEARHAUS_CLIENT_SECRET")?,
            token: String::new(),
            expires_at: Utc::now(),
            health_url: parse_env("CLEARHAUS_HEALTHCHECK_URL")?,
            http,
            db,
        })
    }

    async fn ensure_token(&mut self) -> Result<()> {
        if Utc::now() < self.expires_at - Cd::minutes(15) && !self.token.is_empty() {
            return Ok(());
        }

        log::trace!("Refreshing auth token");
        let r = self
            .http
            .post(format!("{}/oauth/token", self.base_url))
            .basic_auth(&self.client_id, Some(&self.client_secret))
            .form(&[
                ("grant_type", "client_credentials"),
                ("audience", &self.base_url),
            ])
            .send()
            .await?;
        let j = r.json::<Value>().await?;

        self.token = j["access_token"]
            .as_str()
            .ok_or(anyhow!("Error parsing token from {j}"))?
            .to_string();

        self.expires_at = Utc::now()
            + Cd::seconds(
                j["expires_in"]
                    .as_i64()
                    .ok_or(anyhow!("Error parsing token expiration from {j}"))?,
            );

        Ok(())
    }

    async fn fetch(&mut self) -> Result<Value> {
        self.ensure_token().await?;

        log::trace!("Fetching settlements");
        let r = self
            .http
            .get(format!(
                "{}/settlements?query=settled%3Afalse",
                self.base_url
            ))
            .bearer_auth(&self.token)
            .send()
            .await?;

        if !r.status().is_success() {
            return Err(anyhow!(
                "Error fetching settlements: {} {}",
                r.status(),
                r.text().await?
            ));
        }

        r.json().await.map_err(Into::into)
    }

    async fn process(&self, settlements: Value) -> Result<()> {
        log::trace!("Processing response");
        for s in settlements["_embedded"]["ch:settlements"]
            .as_array()
            .ok_or(anyhow!(
                "Error parsing settlements array from {settlements}"
            ))?
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
            .execute(&self.db)
            .await
            {
                Ok(_) => log::debug!("OK merchant_id={merchant_id} amount={amount}"),
                Err(err) => log::error!(
                    "Error saving into DB merchant_id={merchant_id} value={amount}: {err}"
                ),
            }
        }

        Ok(())
    }
}

impl CronJob for Task {
    fn run_once<'a>(&'a mut self) -> BoxFuture<'a, Result<()>> {
        async move {
            let data = self.fetch().await?;
            self.process(data).await?;
            healthy(&self.http, &self.health_url, "clearhaus").await;
            Ok(())
        }
        .boxed()
    }
}
