use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
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

/// withdraw request (legacy API)
#[derive(Debug, Serialize)]
struct WithdrawRequest {
    method: String,
    coin: String,
    to: String,
    amount: String,
    userpass: String,
}

/// withdraw response (legacy API)
#[derive(Debug, Deserialize, Clone)]
pub struct WithdrawResponse {
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
}

pub async fn withdraw(userpass: &str, coin: &str, to: &str, amount: &str) -> Result<WithdrawResponse> {
    let client = reqwest::Client::new();
    let request = WithdrawRequest {
        method: "withdraw".to_string(),
        coin: coin.to_string(),
        to: to.to_string(),
        amount: amount.to_string(),
        userpass: userpass.to_string(),
    };
    let response = client
        .post(KDF_RPC_URL)
        .json(&request)
        .send()
        .await
        .context("Failed to send withdraw request to KDF")?;
    let text = response
        .text()
        .await
        .context("Failed to read withdraw response body")?;
    log_rpc_response("withdraw", &text);
    // Check for error response
    if let Ok(err) = serde_json::from_str::<Value>(&text) {
        if let Some(error) = err.get("error") {
            anyhow::bail!("{}", error);
        }
    }
    let body: WithdrawResponse =
        serde_json::from_str(&text).context("Failed to parse withdraw response from KDF")?;
    Ok(body)
}

/// orderbook request (legacy API)
#[derive(Debug, Serialize)]
struct OrderbookRequest {
    method: String,
    base: String,
    rel: String,
    userpass: String,
}

/// Price field in orderbook entry (we only need the decimal string).
#[derive(Debug, Deserialize, Clone)]
pub struct OrderbookDecimal {
    pub decimal: String,
}

/// A single order entry (ask or bid) in the orderbook response.
#[derive(Debug, Deserialize, Clone)]
pub struct OrderbookEntry {
    pub coin: String,
    pub price: OrderbookDecimal,
    pub base_max_volume: OrderbookDecimal,
    pub base_min_volume: OrderbookDecimal,
    pub rel_max_volume: OrderbookDecimal,
    pub rel_min_volume: OrderbookDecimal,
    pub uuid: String,
    pub is_mine: bool,
    pub pubkey: String,
}

/// Orderbook response result.
#[derive(Debug, Deserialize, Clone)]
pub struct OrderbookResult {
    pub asks: Vec<OrderbookEntry>,
    pub bids: Vec<OrderbookEntry>,
    pub base: String,
    pub rel: String,
    pub num_asks: u32,
    pub num_bids: u32,
    pub timestamp: u64,
    pub total_asks_base_vol: OrderbookDecimal,
    pub total_asks_rel_vol: OrderbookDecimal,
    pub total_bids_base_vol: OrderbookDecimal,
    pub total_bids_rel_vol: OrderbookDecimal,
}

/// Orderbook response (mmrpc 2.0 format).
#[derive(Debug, Deserialize)]
pub struct OrderbookResponse {
    pub result: OrderbookResult,
}

pub async fn orderbook(userpass: &str, base: &str, rel: &str) -> Result<OrderbookResponse> {
    let client = reqwest::Client::new();
    let request = serde_json::json!({
        "method": "orderbook",
        "mmrpc": "2.0",
        "params": {
            "base": base,
            "rel": rel
        },
        "userpass": userpass
    });
    let response = client
        .post(KDF_RPC_URL)
        .json(&request)
        .send()
        .await
        .context("Failed to send orderbook request to KDF")?;
    let text = response
        .text()
        .await
        .context("Failed to read orderbook response body")?;
    log_rpc_response("orderbook", &text);
    // Check for error response
    if let Ok(err) = serde_json::from_str::<Value>(&text) {
        if let Some(error) = err.get("error") {
            anyhow::bail!("{}", error);
        }
    }
    let body: OrderbookResponse =
        serde_json::from_str(&text).context("Failed to parse orderbook response from KDF")?;
    Ok(body)
}

// --- setprice (maker order) ---

/// Confirmation settings in order responses.
#[derive(Debug, Deserialize, Clone)]
pub struct ConfSettings {
    pub base_confs: u32,
    pub base_nota: bool,
    pub rel_confs: u32,
    pub rel_nota: bool,
}

/// setprice response result.
#[derive(Debug, Deserialize, Clone)]
pub struct SetPriceResult {
    pub base: String,
    pub rel: String,
    pub price: String,
    pub max_base_vol: String,
    pub min_base_vol: String,
    pub created_at: u64,
    pub uuid: String,
    pub conf_settings: Option<ConfSettings>,
}

/// setprice response.
#[derive(Debug, Deserialize)]
pub struct SetPriceResponse {
    pub result: SetPriceResult,
}

pub async fn setprice(
    userpass: &str,
    base: &str,
    rel: &str,
    price: &str,
    volume: &str,
    base_confs: u32,
    base_nota: bool,
    rel_confs: u32,
    rel_nota: bool,
) -> Result<SetPriceResponse> {
    let client = reqwest::Client::new();
    let request = serde_json::json!({
        "method": "setprice",
        "userpass": userpass,
        "base": base,
        "rel": rel,
        "price": price,
        "volume": volume,
        "base_confs": base_confs,
        "base_nota": base_nota,
        "rel_confs": rel_confs,
        "rel_nota": rel_nota,
    });
    let response = client
        .post(KDF_RPC_URL)
        .json(&request)
        .send()
        .await
        .context("Failed to send setprice request to KDF")?;
    let text = response
        .text()
        .await
        .context("Failed to read setprice response body")?;
    log_rpc_response("setprice", &text);
    if let Ok(err) = serde_json::from_str::<Value>(&text) {
        if let Some(error) = err.get("error") {
            anyhow::bail!("{}", error);
        }
    }
    let body: SetPriceResponse =
        serde_json::from_str(&text).context("Failed to parse setprice response from KDF")?;
    Ok(body)
}

// --- my_orders ---

/// A single maker order from my_orders response.
#[derive(Debug, Deserialize, Clone)]
pub struct MyMakerOrder {
    pub base: String,
    pub rel: String,
    pub price: String,
    pub max_base_vol: String,
    pub min_base_vol: String,
    pub created_at: u64,
    pub uuid: String,
    pub conf_settings: Option<ConfSettings>,
    #[serde(default)]
    pub cancellable: bool,
    #[serde(default)]
    pub available_amount: Option<String>,
}

/// A single taker order from my_orders response.
#[derive(Debug, Deserialize, Clone)]
pub struct MyTakerOrder {
    pub created_at: u64,
    pub request: TakerOrderRequest,
    #[serde(default)]
    pub cancellable: bool,
    #[serde(default)]
    pub order_type: Option<String>,
}

/// Taker order request details.
#[derive(Debug, Deserialize, Clone)]
pub struct TakerOrderRequest {
    pub base: String,
    pub rel: String,
    pub base_amount: String,
    pub rel_amount: String,
    pub action: String,
    pub uuid: String,
}

/// my_orders response result.
#[derive(Debug, Deserialize)]
pub struct MyOrdersResult {
    pub maker_orders: HashMap<String, MyMakerOrder>,
    pub taker_orders: HashMap<String, MyTakerOrder>,
}

/// my_orders response.
#[derive(Debug, Deserialize)]
pub struct MyOrdersResponse {
    pub result: MyOrdersResult,
}

pub async fn my_orders(userpass: &str) -> Result<MyOrdersResponse> {
    let client = reqwest::Client::new();
    let request = serde_json::json!({
        "method": "my_orders",
        "userpass": userpass,
    });
    let response = client
        .post(KDF_RPC_URL)
        .json(&request)
        .send()
        .await
        .context("Failed to send my_orders request to KDF")?;
    let text = response
        .text()
        .await
        .context("Failed to read my_orders response body")?;
    log_rpc_response("my_orders", &text);
    if let Ok(err) = serde_json::from_str::<Value>(&text) {
        if let Some(error) = err.get("error") {
            anyhow::bail!("{}", error);
        }
    }
    let body: MyOrdersResponse =
        serde_json::from_str(&text).context("Failed to parse my_orders response from KDF")?;
    Ok(body)
}

// --- order_status ---

#[derive(Debug, Deserialize, Clone)]
pub struct OrderStatusMakerOrder {
    pub base: String,
    pub rel: String,
    pub price: String,
    pub max_base_vol: String,
    pub min_base_vol: String,
    #[serde(default)]
    pub available_amount: Option<String>,
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: Option<u64>,
    pub uuid: String,
    #[serde(default)]
    pub cancellable: bool,
    #[serde(default)]
    pub started_swaps: Vec<String>,
    pub conf_settings: Option<ConfSettings>,
    #[serde(default)]
    pub cancellation_reason: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct OrderStatusTakerRequest {
    pub base: String,
    pub rel: String,
    pub base_amount: String,
    pub rel_amount: String,
    pub action: String,
    pub uuid: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct OrderStatusTakerOrderType {
    #[serde(rename = "type")]
    pub order_type: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct OrderStatusTakerOrder {
    pub created_at: u64,
    pub request: OrderStatusTakerRequest,
    #[serde(default)]
    pub cancellable: bool,
    pub order_type: Option<OrderStatusTakerOrderType>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum OrderStatusResponse {
    Maker {
        order: OrderStatusMakerOrder,
    },
    Taker {
        order: OrderStatusTakerOrder,
        #[serde(default)]
        cancellation_reason: Option<String>,
    },
}

pub async fn order_status(userpass: &str, uuid: &str) -> Result<OrderStatusResponse> {
    let client = reqwest::Client::new();
    let request = serde_json::json!({
        "method": "order_status",
        "userpass": userpass,
        "uuid": uuid,
    });
    let response = client
        .post(KDF_RPC_URL)
        .json(&request)
        .send()
        .await
        .context("Failed to send order_status request to KDF")?;
    let text = response
        .text()
        .await
        .context("Failed to read order_status response body")?;
    log_rpc_response("order_status", &text);
    if let Ok(err) = serde_json::from_str::<Value>(&text) {
        if let Some(error) = err.get("error") {
            anyhow::bail!("{}", error);
        }
    }
    let body: OrderStatusResponse =
        serde_json::from_str(&text).context("Failed to parse order_status response from KDF")?;
    Ok(body)
}

/// send_raw_transaction request (legacy API)
#[derive(Debug, Serialize)]
struct SendRawTransactionRequest {
    method: String,
    coin: String,
    tx_hex: String,
    userpass: String,
}

/// send_raw_transaction response (legacy API)
#[derive(Debug, Deserialize)]
pub struct SendRawTransactionResponse {
    pub tx_hash: String,
}

pub async fn send_raw_transaction(userpass: &str, coin: &str, tx_hex: &str) -> Result<SendRawTransactionResponse> {
    let client = reqwest::Client::new();
    let request = SendRawTransactionRequest {
        method: "send_raw_transaction".to_string(),
        coin: coin.to_string(),
        tx_hex: tx_hex.to_string(),
        userpass: userpass.to_string(),
    };
    let response = client
        .post(KDF_RPC_URL)
        .json(&request)
        .send()
        .await
        .context("Failed to send send_raw_transaction request to KDF")?;
    let text = response
        .text()
        .await
        .context("Failed to read send_raw_transaction response body")?;
    log_rpc_response("send_raw_transaction", &text);
    // Check for error response
    if let Ok(err) = serde_json::from_str::<Value>(&text) {
        if let Some(error) = err.get("error") {
            anyhow::bail!("{}", error);
        }
    }
    let body: SendRawTransactionResponse =
        serde_json::from_str(&text).context("Failed to parse send_raw_transaction response from KDF")?;
    Ok(body)
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
