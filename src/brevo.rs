use crate::{
    cron::{CronJob, spawn_cron},
    helpers::parse_env,
};
use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use futures::{FutureExt, future::BoxFuture};
use itertools::Itertools;
use num_traits::ToPrimitive;
use reqwest::Client;
use serde_json::{Value, json};
use sqlx::{PgPool, query, types::BigDecimal};
use std::time::Duration;

pub fn start(http: Client, db: PgPool) {
    match parse_env("BREVO_INTERVAL_SECONDS").and_then(|v| v.parse().map_err(Into::into)) {
        Ok(interval_secs) => {
            spawn_cron(
                "brevo",
                Duration::from_secs(interval_secs),
                Task::new(http, db).expect("Invalid config"),
            );
        }
        Err(_) => log::warn!("[brevo] disabled (no/invalid interval)"),
    };
}

struct Task {
    brevo_api_url: String,
    brevo_api_key: String,
    renew_payment_url: String,
    list_id: i64,
    health_url: String,
    http: Client,
    db: PgPool,
}

impl Task {
    fn new(http: Client, db: PgPool) -> Result<Self> {
        Ok(Self {
            brevo_api_url: parse_env("BREVO_API_URL")?,
            brevo_api_key: parse_env("BREVO_API_KEY")?,
            renew_payment_url: parse_env("BREVO_RENEW_PAYMENT_URL")?,
            list_id: parse_env("BREVO_LIST_ID")?.parse()?,
            health_url: parse_env("BREVO_HEALTHCHECK_URL")?,
            http,
            db,
        })
    }

    async fn payload(&self) -> Result<Vec<Value>> {
        let to_date = |d: Option<DateTime<Utc>>| d.map(|x| x.format("%Y-%m-%d").to_string());
        let to_number = |n: Option<BigDecimal>| n.map(|x| x.to_f64());
        let to_link = |id: Option<String>| id.map(|v| format!("{}?id={v}", self.renew_payment_url));

        log::trace!("Fetching data from db");
        let entries = query!(
            "
                select
                    email,
                    registered_at,
                    name,
                    cvr,
                    age,
                    total_donated,
                    donations_count,
                    last_donated_amount,
                    last_donated_method::text,
                    last_donated_frequency::text,
                    last_donated_recipient::text,
                    last_donation_tax_deductible,
                    last_donation_cancelled,
                    last_donated_at,
                    first_membership_at,
                    first_donation_at,
                    first_monthly_donation_at,
                    is_member,
                    has_gavebrev,
                    vitamin_a_amount,
                    vitamin_a_units,
                    vaccinations_amount,
                    vaccinations_units,
                    bednets_amount,
                    bednets_units,
                    malaria_medicine_amount,
                    malaria_medicine_units,
                    direct_transfer_amount,
                    direct_transfer_units,
                    deworming_amount,
                    deworming_units,
                    lives,
                    expired_donation_id::text,
                    expired_donation_at,
                    expired_membership_id::text,
                    expired_membership_at
                from
                    crm_export
                "
        )
        .fetch_all(&self.db)
        .await
        .with_context(|| "Error fetching data for export")?;

        log::trace!("Preparing upload payload");
        entries
            .into_iter()
            .map(|entry| -> Result<_> {
                let mut attributes = json!({
                    "DATO_FOR_OPRETTELSE": to_date(entry.registered_at),
                    "SIDSTE_DONATIONSDATO": to_date(entry.last_donated_at),
                    "SIDSTE_DONATIONSBELOB": to_number(entry.last_donated_amount),
                    "SIDSTE_DONATIONSFREKVENS": entry.last_donated_frequency,
                    "SIDSTE_DONATIONSMETODE": entry.last_donated_method,
                    "SIDSTE_DONATIONSOEREMAERKNING": entry.last_donated_recipient,
                    "ER_SIDSTE_DONATION_OPSAGT": entry.last_donation_cancelled,
                    "ER_SIDSTE_DONATION_FRADRAGSBERETTIGET": entry.last_donation_tax_deductible,
                    "FOERSTE_DONATIONSDATO": to_date(entry.first_donation_at),
                    "FOERSTE_MAANEDLIG_DONATIONSDATO": to_date(entry.first_monthly_donation_at),
                    "FOERSTE_MEDLEMSKABSDATO": to_date(entry.first_membership_at),
                    "TOTALT_DONERET": to_number(entry.total_donated),
                    "ANTAL_DONATIONER": entry.donations_count,
                    "MEDLEM": entry.is_member,
                    "GAVEBREV": entry.has_gavebrev,
                    "ALDER": entry.age,
                    "CVR": entry.cvr,
                    "DONERET_AVITAMIN": to_number(entry.vitamin_a_amount),
                    "IMPACT_AVITAMIN": to_number(entry.vitamin_a_units),
                    "DONERET_INCENTIVES": to_number(entry.vaccinations_amount),
                    "IMPACT_INCENTIVES": to_number(entry.vaccinations_units),
                    "DONERET_MYGGENET": to_number(entry.bednets_amount),
                    "IMPACT_MYGGENET": to_number(entry.bednets_units),
                    "DONERET_MALARIAMEDICIN": to_number(entry.malaria_medicine_amount),
                    "IMPACT_MALARIAMEDICIN": to_number(entry.malaria_medicine_units),
                    "DONERET_KONTANTOVERFOERSLER": to_number(entry.direct_transfer_amount),
                    "IMPACT_KONTANTOVERFOERSLER": to_number(entry.direct_transfer_units),
                    "DONERET_ORMEKURE": to_number(entry.deworming_amount),
                    "IMPACT_ORMEKURE": to_number(entry.deworming_units),
                    "LIVES_SAVED": to_number(entry.lives),
                    "UDLOEBET_DONATIONSLINK": to_link(entry.expired_donation_id),
                    "DONATIONENS_UDLOEBSDATO": to_date(entry.expired_donation_at),
                    "UDLOEBET_MEDLEMSKABSLINK": to_link(entry.expired_membership_id),
                    "MEDLEMSKABETS_UDLOEBSDATO": to_date(entry.expired_membership_at),
                });

                if let Some(name) = entry.name {
                    attributes
                        .as_object_mut()
                        .ok_or(anyhow!("Error adding name to attributes"))?
                        .insert("FIRSTNAME".to_string(), json!(name));
                }

                Ok(json!({
                    "email": entry.email,
                    "attributes": attributes,
                }))
            })
            .try_collect()
    }

    async fn upload(&self, payload: Vec<Value>) -> Result<()> {
        log::trace!("Uploading to Brevo");
        let url = format!("{}/contacts/import", self.brevo_api_url);
        let body = json!({
            "disableNotification": true,
            "updateExistingContacts": true,
            "emptyContactsAttributes": true,
            "jsonBody": payload,
            "listIds": vec![self.list_id],
        });

        let response = self
            .http
            .post(url)
            .header("api-key", &self.brevo_api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Error uploading to Brevo {}: {}",
                response.status(),
                response.text().await?
            ));
        }

        Ok(())
    }

    async fn healthy(&self) -> Result<()> {
        log::trace!("Reporting health");
        let h = self.http.post(&self.health_url).send().await?;
        if !h.status().is_success() {
            return Err(anyhow!(
                "Error reporting health: {} {}",
                h.status(),
                h.text().await?
            ));
        }
        Ok(())
    }
}

impl CronJob for Task {
    fn run_once<'a>(&'a mut self) -> BoxFuture<'a, Result<()>> {
        async move {
            let data = self.payload().await?;
            self.upload(data).await?;
            self.healthy().await
        }
        .boxed()
    }
}
