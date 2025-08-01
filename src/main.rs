use anyhow::Result;
use reqwest::Client;
use sqlx::PgPool;
use std::time::Duration;

use crate::helpers::parse_env;

mod clearhaus;
mod helpers;
// mod crm;
mod cron;
mod metrics;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .parse_default_env()
        .init();

    let db = PgPool::connect(&parse_env("DATABASE_URL")?).await?;
    let http = Client::builder()
        .use_rustls_tls()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .tcp_keepalive(Duration::from_secs(30))
        .build()?;

    prometheus_exporter::start("0.0.0.0:9090".parse()?)?;

    clearhaus::start(http.clone(), db.clone());
    // crm::start(http.clone(), db.clone());

    std::panic::set_hook(Box::new(|_| {}));
    tokio::signal::ctrl_c().await?;
    Ok(())
}
