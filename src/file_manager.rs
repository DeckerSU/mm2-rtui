use anyhow::{Context, Result};
use reqwest;
use std::path::{Path, PathBuf};
use tokio::fs;
use crate::logger::SharedLogger;

const COINS_CONFIG_URL: &str = "https://raw.githubusercontent.com/KomodoPlatform/coins/refs/heads/master/utils/coins_config_unfiltered.json";
const COINS_URL: &str = "https://raw.githubusercontent.com/KomodoPlatform/coins/refs/heads/master/coins";
const SEED_NODES_URL: &str = "https://raw.githubusercontent.com/KomodoPlatform/coins/refs/heads/master/seed-nodes.json";

const COINS_CONFIG_ALT_URL: &str = "https://komodoplatform.github.io/coins/utils/coins_config_unfiltered.json";
const COINS_ALT_URL: &str = "https://komodoplatform.github.io/coins/coins";
const SEED_NODES_ALT_URL: &str = "https://komodoplatform.github.io/coins/seed-nodes.json";

pub async fn ensure_required_files(workspace_path: &Path, logger: &SharedLogger) -> Result<()> {
    // Check and download coins_config.json
    let coins_config_path = workspace_path.join("coins_config.json");
    if !coins_config_path.exists() {
        if let Ok(mut log) = logger.write() {
            log.info(format!("Downloading coins_config.json from {}", COINS_CONFIG_URL));
        }
        download_file(COINS_CONFIG_URL, &coins_config_path, COINS_CONFIG_ALT_URL, logger).await?;
        if let Ok(mut log) = logger.write() {
            log.info(format!("Successfully downloaded coins_config.json"));
        }
    } else {
        if let Ok(mut log) = logger.write() {
            log.info("coins_config.json already exists".to_string());
        }
    }
    
    // Check and download coins.json
    let coins_path = workspace_path.join("coins.json");
    if !coins_path.exists() {
        if let Ok(mut log) = logger.write() {
            log.info(format!("Downloading coins.json from {}", COINS_URL));
        }
        download_file(COINS_URL, &coins_path, COINS_ALT_URL, logger).await?;
        if let Ok(mut log) = logger.write() {
            log.info(format!("Successfully downloaded coins.json"));
        }
    } else {
        if let Ok(mut log) = logger.write() {
            log.info("coins.json already exists".to_string());
        }
    }
    
    // Check and download seed-nodes.json
    let seed_nodes_path = workspace_path.join("seed-nodes.json");
    if !seed_nodes_path.exists() {
        if let Ok(mut log) = logger.write() {
            log.info(format!("Downloading seed-nodes.json from {}", SEED_NODES_URL));
        }
        download_file(SEED_NODES_URL, &seed_nodes_path, SEED_NODES_ALT_URL, logger).await?;
        if let Ok(mut log) = logger.write() {
            log.info(format!("Successfully downloaded seed-nodes.json"));
        }
    } else {
        if let Ok(mut log) = logger.write() {
            log.info("seed-nodes.json already exists".to_string());
        }
    }
    
    Ok(())
}

async fn download_file(primary_url: &str, path: &PathBuf, fallback_url: &str, logger: &SharedLogger) -> Result<()> {
    let client = reqwest::Client::new();
    
    // Try primary URL first
    let response = client.get(primary_url).send().await;
    
    let response = match response {
        Ok(resp) if resp.status().is_success() => resp,
        _ => {
            // Try fallback URL
            if let Ok(mut log) = logger.write() {
                log.warn(format!("Primary URL failed, trying fallback: {}", fallback_url));
            }
            client
                .get(fallback_url)
                .send()
                .await
                .context("Failed to download file from both URLs")?
        }
    };
    
    let content = response
        .text()
        .await
        .context("Failed to read response body")?;
    
    fs::write(path, content)
        .await
        .with_context(|| format!("Failed to write file: {}", path.display()))?;
    
    Ok(())
}
