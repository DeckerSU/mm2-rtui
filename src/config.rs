use anyhow::{Context, Result};
use directories::{BaseDirs, UserDirs};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tokio::fs;
use crate::logger::SharedLogger;
use rand::Rng;

/// Update wallet-related fields in existing MM2.json and sync to disk.
pub async fn update_mm2_wallet(
    mm2_path: &Path,
    wallet_name: &str,
    wallet_password: &str,
    enable_hd: bool,
    logger: &SharedLogger,
) -> Result<()> {
    let content = fs::read_to_string(mm2_path)
        .await
        .context("Failed to read MM2.json for wallet update")?;
    let mut config: Value = serde_json::from_str(&content)
        .context("Failed to parse MM2.json")?;

    let obj = config
        .as_object_mut()
        .context("MM2.json root is not an object")?;
    obj.insert("wallet_name".to_string(), Value::String(wallet_name.to_string()));
    obj.insert("wallet_password".to_string(), Value::String(wallet_password.to_string()));
    obj.insert("enable_hd".to_string(), Value::Bool(enable_hd));

    let new_content = serde_json::to_string_pretty(&config)
        .context("Failed to serialize MM2.json")?;
    fs::write(mm2_path, new_content)
        .await
        .context("Failed to write MM2.json")?;

    let file = tokio::fs::File::open(mm2_path).await
        .context("Failed to open MM2.json for sync")?;
    file.sync_all().await
        .context("Failed to sync MM2.json to disk")?;

    if let Ok(mut log) = logger.write() {
        log.info(format!("Updated MM2.json with wallet '{}', enable_hd: {}", wallet_name, enable_hd));
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct SeedNode {
    name: String,
    host: String,
    #[serde(rename = "type")]
    node_type: String,
    wss: bool,
    netid: u64,
    contact: Vec<Value>,
}

fn generate_secure_password() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*";
    const PASSWORD_LENGTH: usize = 16;
    
    let mut rng = rand::thread_rng();
    let password: String = (0..PASSWORD_LENGTH)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect();
    
    password
}

pub async fn setup_mm2_config(mm2_path: &PathBuf, workspace_path: &Path, logger: &SharedLogger) -> Result<String> {
    // Try to read existing MM2.json and reuse rpc_password if present
    let rpc_password = if mm2_path.exists() {
        match fs::read_to_string(mm2_path).await {
            Ok(content) => {
                if let Ok(existing) = serde_json::from_str::<Value>(&content) {
                    if let Some(Value::String(pw)) = existing.get("rpc_password") {
                        if !pw.is_empty() {
                            if let Ok(mut log) = logger.write() {
                                log.info(format!("Using existing RPC password from {}", mm2_path.display()));
                            }
                            pw.clone()
                        } else {
                            let pw = generate_secure_password();
                            if let Ok(mut log) = logger.write() {
                                log.info(format!("Existing rpc_password empty, generated new: {}", pw));
                            }
                            pw
                        }
                    } else {
                        let pw = generate_secure_password();
                        if let Ok(mut log) = logger.write() {
                            log.info(format!("No rpc_password in existing config, generated: {}", pw));
                        }
                        pw
                    }
                } else {
                    let pw = generate_secure_password();
                    if let Ok(mut log) = logger.write() {
                        log.info(format!("Could not parse existing MM2.json, generated RPC password: {}", pw));
                    }
                    pw
                }
            }
            Err(_) => {
                let pw = generate_secure_password();
                if let Ok(mut log) = logger.write() {
                    log.info(format!("Could not read existing MM2.json, generated RPC password: {}", pw));
                }
                pw
            }
        }
    } else {
        let pw = generate_secure_password();
        if let Ok(mut log) = logger.write() {
            log.info(format!("No existing MM2.json, generated RPC password: {}", pw));
        }
        pw
    };

    // Read seed-nodes.json
    let seed_nodes_path = workspace_path.join("seed-nodes.json");
    if let Ok(mut log) = logger.write() {
        log.info(format!("Reading seed-nodes.json from {}", seed_nodes_path.display()));
    }
    
    let seed_nodes_content = fs::read_to_string(&seed_nodes_path)
        .await
        .context("Failed to read seed-nodes.json")?;
    
    let seed_nodes: Vec<SeedNode> = serde_json::from_str(&seed_nodes_content)
        .context("Failed to parse seed-nodes.json")?;
    
    // Extract hosts
    let seed_hosts: Vec<String> = seed_nodes
        .iter()
        .map(|node| node.host.clone())
        .collect();
    
    if let Ok(mut log) = logger.write() {
        log.info(format!("Found {} seed nodes", seed_hosts.len()));
    }

    // userhome: $HOME + system documents folder; fallback $HOME/Documents
    let home = BaseDirs::new()
        .map(|b| b.home_dir().to_path_buf())
        .or_else(|| std::env::var("HOME").ok().map(PathBuf::from))
        .context("Could not determine home directory")?;
    let userhome = UserDirs::new()
        .and_then(|u| u.document_dir().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| home.join("Documents"));
    fs::create_dir_all(&userhome)
        .await
        .context("Failed to create userhome directory")?;
    let dbdir = userhome.join(".kdf");
    fs::create_dir_all(&dbdir)
        .await
        .context("Failed to create dbdir directory")?;
    let userhome_str = userhome.to_string_lossy().into_owned();
    let dbdir_str = dbdir.to_string_lossy().into_owned();
    if let Ok(mut log) = logger.write() {
        log.info(format!("userhome: {}", userhome_str));
        log.info(format!("dbdir: {}", dbdir_str));
    }

    // Create MM2.json content
    let mm2_config = json!({
        "mm2": 1,
        "gui": "mm2-rtui",
        "rpcport": 7783,
        "netid": 8762, // 6133
        "https": false,
        "wallet_name": null,
        "wallet_password": null,
        "seed": null,
        "allowRegistrations": false,
        "enable_hd": false,
        "rpcLocalOnly": true,
        "rpc_password": rpc_password.clone(),
        "seednodes": seed_hosts,
        "userhome": userhome_str,
        "dbdir": dbdir_str
    });
    
    // Write MM2.json
    let mm2_content = serde_json::to_string_pretty(&mm2_config)
        .context("Failed to serialize MM2.json")?;
    
    fs::write(mm2_path, mm2_content)
        .await
        .context("Failed to write MM2.json")?;
    
    // Ensure file is synced to disk
    use tokio::fs::File;
    let file = File::open(mm2_path).await
        .context("Failed to open MM2.json for sync")?;
    file.sync_all().await
        .context("Failed to sync MM2.json to disk")?;
    
    if let Ok(mut log) = logger.write() {
        log.info(format!("Successfully created/updated MM2.json at {}", mm2_path.display()));
        log.info(format!("MM2.json synced to disk with RPC password"));
    }
    
    Ok(rpc_password)
}
