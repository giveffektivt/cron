use anyhow::Result;
use metrics_exporter_prometheus::PrometheusBuilder;
use reqwest::Client;
use sqlx::PgPool;
use std::time::Duration;

use crate::helpers::parse_env;

mod brevo;
mod clearhaus;
mod cron;
mod helpers;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .parse_default_env()
        .init();

    let db = PgPool::connect(&parse_env("DATABASE_URL")?).await?;
    let http = Client::builder()
        .use_rustls_tls()
        .timeout(Duration::from_secs(60))
        .build()?;

    PrometheusBuilder::new().install()?;

    clearhaus::start(http.clone(), db.clone());
    brevo::start(http.clone(), db.clone());

    std::panic::set_hook(Box::new(|_| {}));
    tokio::signal::ctrl_c().await?;
    Ok(())
}
