use anyhow::Result;
use reqwest::Client;
use sqlx::PgPool;
use std::env;

use crate::clearhaus::clearhaus;

mod clearhaus;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .parse_default_env()
        .init();

    let pool = PgPool::connect(&env::var("DATABASE_URL")?).await?;
    let http = Client::new();

    tokio::spawn(clearhaus(http.clone(), pool.clone()));

    tokio::signal::ctrl_c().await?;
    Ok(())
}
