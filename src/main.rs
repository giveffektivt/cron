use anyhow::Result;
use reqwest::Client;
use sqlx::PgPool;
use std::{env, time::Duration};

mod clearhaus;
mod helpers;
// mod crm;
mod cron;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .parse_default_env()
        .init();

    let db = PgPool::connect(&env::var("DATABASE_URL")?).await?;
    let http = Client::builder()
        .use_rustls_tls()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .tcp_keepalive(Duration::from_secs(30))
        .build()?;

    clearhaus::start(http.clone(), db.clone());
    // crm::start(http.clone(), db.clone());

    std::panic::set_hook(Box::new(|_| {}));
    tokio::signal::ctrl_c().await?;
    Ok(())
}
