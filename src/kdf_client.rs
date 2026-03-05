use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Write;

const KDF_RPC_URL: &str = "http://127.0.0.1:7783";
const DEBUG_LOG_PATH: &str = "debug.log";

/// Append RPC method and raw response to debug.log.
fn log_rpc_response(method: &str, response: &str) {
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let mut line = format!("[{}] RPC {} response:\n{}\n", timestamp, method, response);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .write(true)
        .open(DEBUG_LOG_PATH)
    {
        let _ = f.write_all(line.as_bytes());
        let _ = f.flush();
    }
}

#[derive(Debug, Serialize)]
struct VersionRequest {
    method: String,
    userpass: String,
}

#[derive(Debug, Serialize)]
struct StopRequest {
    method: String,
    userpass: String,
}

#[derive(Debug, Deserialize)]
pub struct VersionResponse {
    pub result: String,
    pub datetime: String,
}

#[derive(Debug, Deserialize)]
pub struct StopResponse {
    pub result: String,
}

#[derive(Debug, Serialize)]
struct MyBalanceRequest {
    method: String,
    coin: String,
    userpass: String,
}

#[derive(Debug, Deserialize)]
pub struct MyBalanceResponse {
    pub coin: String,
    pub balance: String,
    pub unspendable_balance: String,
    pub address: String,
}

pub async fn my_balance(userpass: &str, coin: &str) -> Result<MyBalanceResponse> {
    let client = reqwest::Client::new();
    let request = MyBalanceRequest {
        method: "my_balance".to_string(),
        coin: coin.to_string(),
        userpass: userpass.to_string(),
    };
    let response = client
        .post(KDF_RPC_URL)
        .json(&request)
        .send()
        .await
        .context("Failed to send my_balance request to KDF")?;
    let text = response
        .text()
        .await
        .context("Failed to read my_balance response body")?;
    log_rpc_response("my_balance", &text);
    let body: MyBalanceResponse =
        serde_json::from_str(&text).context("Failed to parse my_balance response from KDF")?;
    Ok(body)
}

/// Request for get_wallet_names (mmrpc 2.0 format).
#[derive(Debug, Serialize)]
pub struct GetWalletNamesRequest {
    pub method: String,
    pub mmrpc: String,
    pub params: Option<()>,
    pub userpass: String,
    pub id: u64,
}

#[derive(Debug, Deserialize)]
pub struct GetWalletNamesResult {
    pub wallet_names: Vec<String>,
    #[allow(dead_code)]
    pub activated_wallet: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GetWalletNamesResponse {
    pub result: GetWalletNamesResult,
    #[allow(dead_code)]
    pub id: u64,
}

pub async fn get_wallet_names(userpass: &str) -> Result<GetWalletNamesResponse> {
    let client = reqwest::Client::new();
    let id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let request = GetWalletNamesRequest {
        method: "get_wallet_names".to_string(),
        mmrpc: "2.0".to_string(),
        params: None,
        userpass: userpass.to_string(),
        id,
    };
    let response = client
        .post(KDF_RPC_URL)
        .json(&request)
        .send()
        .await
        .context("Failed to send get_wallet_names request to KDF")?;
    let text = response
        .text()
        .await
        .context("Failed to read get_wallet_names response body")?;
    log_rpc_response("get_wallet_names", &text);
    let body: GetWalletNamesResponse =
        serde_json::from_str(&text).context("Failed to parse get_wallet_names response from KDF")?;
    Ok(body)
}

pub async fn get_version(userpass: &str) -> Result<VersionResponse> {
    let client = reqwest::Client::new();
    
    let request = VersionRequest {
        method: "version".to_string(),
        userpass: userpass.to_string(),
    };
    
    let response = client
        .post(KDF_RPC_URL)
        .json(&request)
        .send()
        .await
        .context("Failed to send version request to KDF")?;
    let text = response
        .text()
        .await
        .context("Failed to read version response body")?;
    log_rpc_response("version", &text);
    let version_info: VersionResponse =
        serde_json::from_str(&text).context("Failed to parse version response from KDF")?;
    Ok(version_info)
}

/// task::enable_utxo::init — returns task_id.
#[derive(Debug, Serialize)]
pub struct TaskEnableUtxoInitRequest {
    pub method: String,
    pub mmrpc: String,
    pub params: Value,
    pub userpass: String,
}

#[derive(Debug, Deserialize)]
pub struct TaskEnableUtxoInitResponse {
    pub result: TaskEnableUtxoInitResult,
}

#[derive(Debug, Deserialize)]
pub struct TaskEnableUtxoInitResult {
    pub task_id: u64,
}

pub async fn task_enable_utxo_init(
    userpass: &str,
    ticker: &str,
    activation_params: Value,
) -> Result<TaskEnableUtxoInitResponse> {
    let client = reqwest::Client::new();
    let params = serde_json::json!({
        "ticker": ticker,
        "activation_params": activation_params
    });
    let request = TaskEnableUtxoInitRequest {
        method: "task::enable_utxo::init".to_string(),
        mmrpc: "2.0".to_string(),
        params,
        userpass: userpass.to_string(),
    };
    let response = client
        .post(KDF_RPC_URL)
        .json(&request)
        .send()
        .await
        .context("Failed to send task::enable_utxo::init to KDF")?;
    let text = response
        .text()
        .await
        .context("Failed to read task::enable_utxo::init response body")?;
    log_rpc_response("task::enable_utxo::init", &text);
    let body: TaskEnableUtxoInitResponse =
        serde_json::from_str(&text).context("Failed to parse task::enable_utxo::init response")?;
    Ok(body)
}

/// task::enable_utxo::status — returns status and details.
#[derive(Debug, Serialize)]
pub struct TaskEnableUtxoStatusRequest {
    pub method: String,
    pub mmrpc: String,
    pub params: TaskEnableUtxoStatusParams,
    pub userpass: String,
}

#[derive(Debug, Serialize)]
pub struct TaskEnableUtxoStatusParams {
    pub task_id: u64,
    pub forget_if_finished: bool,
}

#[derive(Debug, Deserialize)]
pub struct TaskEnableUtxoStatusResponse {
    pub result: TaskEnableUtxoStatusResult,
}

#[derive(Debug, Deserialize)]
pub struct TaskEnableUtxoStatusResult {
    pub status: String,
    #[serde(default)]
    pub details: Value,
}

pub async fn task_enable_utxo_status(
    userpass: &str,
    task_id: u64,
    forget_if_finished: bool,
) -> Result<TaskEnableUtxoStatusResponse> {
    let client = reqwest::Client::new();
    let request = TaskEnableUtxoStatusRequest {
        method: "task::enable_utxo::status".to_string(),
        mmrpc: "2.0".to_string(),
        params: TaskEnableUtxoStatusParams {
            task_id,
            forget_if_finished,
        },
        userpass: userpass.to_string(),
    };
    let response = client
        .post(KDF_RPC_URL)
        .json(&request)
        .send()
        .await
        .context("Failed to send task::enable_utxo::status to KDF")?;
    let text = response
        .text()
        .await
        .context("Failed to read task::enable_utxo::status response body")?;
    log_rpc_response("task::enable_utxo::status", &text);
    let body: TaskEnableUtxoStatusResponse =
        serde_json::from_str(&text).context("Failed to parse task::enable_utxo::status response")?;
    Ok(body)
}

pub async fn stop(userpass: &str) -> Result<StopResponse> {
    let client = reqwest::Client::new();
    
    let request = StopRequest {
        method: "stop".to_string(),
        userpass: userpass.to_string(),
    };
    
    let response = client
        .post(KDF_RPC_URL)
        .json(&request)
        .send()
        .await
        .context("Failed to send stop request to KDF")?;
    let text = response
        .text()
        .await
        .context("Failed to read stop response body")?;
    log_rpc_response("stop", &text);
    let stop_response: StopResponse =
        serde_json::from_str(&text).context("Failed to parse stop response from KDF")?;
    Ok(stop_response)
}

/// my_tx_history request (mmrpc 2.0 format)
#[derive(Debug, Serialize)]
pub struct MyTxHistoryRequest {
    pub method: String,
    pub mmrpc: String,
    pub params: MyTxHistoryParams,
    pub userpass: String,
}

/// Parameters for my_tx_history request
#[derive(Debug, Serialize)]
pub struct MyTxHistoryParams {
    pub coin: String,
    pub limit: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paging_options: Option<PagingOptions>,
}

/// Paging options for my_tx_history
#[derive(Debug, Serialize)]
pub struct PagingOptions {
    #[serde(rename = "PageNumber")]
    pub page_number: u32,
}

/// Target information in my_tx_history response
#[derive(Debug, Deserialize, Clone)]
pub struct Target {
    #[serde(rename = "type")]
    pub target_type: String,
}

/// Transaction details in my_tx_history response (API 2.0)
#[derive(Debug, Deserialize, Clone)]
pub struct Transaction {
    pub tx_hex: String,
    pub tx_hash: String,
    pub from: Vec<String>,
    pub to: Vec<String>,
    pub total_amount: String,
    pub spent_by_me: String,
    pub received_by_me: String,
    pub my_balance_change: String,
    pub block_height: u64,
    pub timestamp: i64,
    pub fee_details: Value,
    pub coin: String,
    pub internal_id: String,
    pub transaction_type: String,
    #[serde(default)]
    pub memo: Option<String>,
    pub confirmations: u32,
}

/// Sync status in my_tx_history response
#[derive(Debug, Deserialize, Clone)]
pub struct SyncStatus {
    pub state: String,
}

/// my_tx_history response result (API 2.0)
#[derive(Debug, Deserialize)]
pub struct MyTxHistoryResult {
    pub coin: String,
    pub target: Target,
    pub current_block: u64,
    pub transactions: Vec<Transaction>,
    pub sync_status: SyncStatus,
    pub limit: u32,
    pub skipped: u32,
    pub total: u32,
    pub total_pages: u32,
    pub paging_options: PagingOptionsResult,
}

/// Paging options in response
#[derive(Debug, Deserialize)]
pub struct PagingOptionsResult {
    #[serde(rename = "PageNumber")]
    pub page_number: u32,
}

/// my_tx_history response (mmrpc 2.0 format)
#[derive(Debug, Deserialize)]
pub struct MyTxHistoryResponse {
    pub mmrpc: String,
    pub result: MyTxHistoryResult,
    pub id: Option<Value>,
}

/// Get transaction history for a coin (API 2.0)
pub async fn my_tx_history(
    userpass: &str,
    coin: &str,
    limit: u32,
    page_number: Option<u32>,
) -> Result<MyTxHistoryResponse> {
    let client = reqwest::Client::new();
    
    let paging_options = page_number.map(|pn| PagingOptions { page_number: pn });
    
    let request = MyTxHistoryRequest {
        method: "my_tx_history".to_string(),
        mmrpc: "2.0".to_string(),
        params: MyTxHistoryParams {
            coin: coin.to_string(),
            limit,
            paging_options,
        },
        userpass: userpass.to_string(),
    };
    
    let response = client
        .post(KDF_RPC_URL)
        .json(&request)
        .send()
        .await
        .context("Failed to send my_tx_history request to KDF")?;
    let text = response
        .text()
        .await
        .context("Failed to read my_tx_history response body")?;
    log_rpc_response("my_tx_history", &text);
    let body: MyTxHistoryResponse =
        serde_json::from_str(&text).context("Failed to parse my_tx_history response from KDF")?;
    Ok(body)
}
