use anyhow::{Context, Result};

pub fn parse_env(key: &str) -> Result<String> {
    std::env::var(key).with_context(|| format!("environment variable '{key}' not set"))
}
