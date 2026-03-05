//! Coin list, config loading from coins_config.json, and UTXO activation params.

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;

/// Default tickers to activate when KDF is ready.
pub const DEFAULT_TICKERS: &[&str] = &["KMD", "BTC"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoinType {
    UTXO,
    ZHTLC,
    Tendermint,
    QTUM,
    EVM,
}

impl CoinType {
    pub fn from_protocol_type(s: &str) -> Option<Self> {
        match s {
            "UTXO" => Some(CoinType::UTXO),
            "ZHTLC" => Some(CoinType::ZHTLC),
            "Tendermint" => Some(CoinType::Tendermint),
            "QTUM" => Some(CoinType::QTUM),
            "EVM" => Some(CoinType::EVM),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Coin {
    pub ticker: String,
    #[allow(dead_code)]
    pub name: String,
    #[allow(dead_code)]
    pub coin_type: CoinType,
    pub activated: bool,
    /// Spendable balance in satoshis (1 COIN = 100_000_000).
    pub spendable_satoshis: Option<i64>,
    /// Unspendable balance in satoshis.
    pub unspendable_satoshis: Option<i64>,
    /// Current block from last status (task::enable_utxo::status).
    pub current_block: Option<u64>,
    /// Wallet type from status details (e.g. "Iguana", "HD").
    pub wallet_type: Option<String>,
    /// Address from status (Iguana wallet).
    pub address: Option<String>,
}

const SATOSHI_PER_COIN: i64 = 100_000_000;

fn satoshis_to_display(sat: i64) -> String {
    let whole = sat / SATOSHI_PER_COIN;
    let frac = (sat % SATOSHI_PER_COIN).unsigned_abs() as u64;
    format!("{}.{:08}", whole, frac)
}

impl Coin {
    /// Total balance (spendable + unspendable) for display in list.
    pub fn balance_display(&self) -> String {
        let spend = self.spendable_satoshis.unwrap_or(0);
        let unspend = self.unspendable_satoshis.unwrap_or(0);
        satoshis_to_display(spend + unspend)
    }

    pub fn spendable_display(&self) -> String {
        satoshis_to_display(self.spendable_satoshis.unwrap_or(0))
    }

    pub fn unspendable_display(&self) -> String {
        satoshis_to_display(self.unspendable_satoshis.unwrap_or(0))
    }
}

#[derive(Debug, Deserialize)]
struct Protocol {
    #[serde(rename = "type")]
    type_: String,
}

#[derive(Debug, Deserialize)]
struct ElectrumServer {
    url: String,
    #[serde(default)]
    protocol: String,
}

#[derive(Debug, Deserialize)]
struct CoinConfigRaw {
    coin: String,
    name: String,
    #[serde(default)]
    protocol: Option<Protocol>,
    #[serde(default)]
    electrum: Option<Vec<ElectrumServer>>,
    #[serde(default)]
    required_confirmations: Option<u32>,
    #[serde(default)]
    requires_notarization: Option<bool>,
    #[serde(default)]
    txfee: Option<u64>,
    #[serde(default)]
    txversion: Option<u32>,
    #[serde(default)]
    pubtype: Option<u8>,
    #[serde(default)]
    p2shtype: Option<u8>,
    #[serde(default)]
    wiftype: Option<u8>,
    #[serde(default)]
    overwintered: Option<u8>,
}

/// Load coins_config.json and return configs for the given tickers.
/// Only returns entries that exist and have protocol.type == "UTXO" (for activation).
pub fn load_utxo_coins_from_config(
    coins_config_path: &Path,
    tickers: &[&str],
) -> Result<Vec<(Coin, Value)>> {
    let content = std::fs::read_to_string(coins_config_path)
        .context("Failed to read coins_config.json")?;
    let configs: HashMap<String, CoinConfigRaw> =
        serde_json::from_str(&content).context("Failed to parse coins_config.json")?;

    let mut out = Vec::new();
    for &ticker in tickers {
        let raw = match configs.get(ticker) {
            Some(r) => r,
            None => continue,
        };
        let proto_type = raw
            .protocol
            .as_ref()
            .map(|p| p.type_.as_str())
            .unwrap_or("");
        let coin_type = CoinType::from_protocol_type(proto_type).unwrap_or(CoinType::UTXO);
        if coin_type != CoinType::UTXO {
            continue;
        }
        let electrum = raw.electrum.as_ref().context(format!(
            "Coin {} has no electrum servers in coins_config.json",
            ticker
        ))?;
        let params = build_utxo_activation_params(ticker, raw, electrum)?;
        let coin = Coin {
            ticker: raw.coin.clone(),
            name: raw.name.clone(),
            coin_type: CoinType::UTXO,
            activated: false,
            spendable_satoshis: None,
            unspendable_satoshis: None,
            current_block: None,
            wallet_type: None,
            address: None,
        };
        out.push((coin, params));
    }
    Ok(out)
}

/// Build params object for task::enable_utxo::init from coin config.
fn build_utxo_activation_params(
    ticker: &str,
    raw: &CoinConfigRaw,
    electrum: &[ElectrumServer],
) -> Result<Value> {
    let servers: Vec<Value> = electrum
        .iter()
        .filter(|e| e.protocol == "TCP" || e.protocol == "SSL")
        .map(|e| {
            json!({
                "url": e.url,
                "protocol": e.protocol,
                "disable_cert_verification": false
            })
        })
        .collect();

    if servers.is_empty() {
        anyhow::bail!(
            "Coin {} has no TCP/SSL electrum servers in coins_config.json",
            ticker
        );
    }

    let activation_params = json!({
        "required_confirmations": raw.required_confirmations.unwrap_or(2),
        "requires_notarization": raw.requires_notarization.unwrap_or(false),
        "priv_key_policy": "ContextPrivKey",
        "min_addresses_number": 1,
        "scan_policy": "scan_if_new_wallet",
        "gap_limit": 20,
        "mode": {
            "rpc": "Electrum",
            "rpc_data": {
                "servers": servers,
                "max_connected": 1
            }
        },
        "tx_history": true,
        "txversion": raw.txversion.unwrap_or(4),
        "txfee": raw.txfee.unwrap_or(1000),
        "pubtype": raw.pubtype.unwrap_or(60),
        "p2shtype": raw.p2shtype.unwrap_or(85),
        "wiftype": raw.wiftype.unwrap_or(188),
        "overwintered": raw.overwintered.unwrap_or(1),
        "max_connected": 1
    });

    Ok(activation_params)
}

/// Parsed fields from task::enable_utxo::status result details.
#[derive(Debug, Clone, Default)]
pub struct CoinStatusDetails {
    pub current_block: Option<u64>,
    pub spendable_satoshis: i64,
    pub unspendable_satoshis: i64,
    pub wallet_type: Option<String>,
    pub address: Option<String>,
}

/// Parse my_balance response balance/unspendable_balance strings to (spendable_satoshis, unspendable_satoshis).
pub fn my_balance_to_satoshis(balance: &str, unspendable_balance: &str) -> (i64, i64) {
    (
        coin_amount_to_satoshis(balance),
        coin_amount_to_satoshis(unspendable_balance),
    )
}

/// Parse decimal coin amount string (e.g. "4269.37384458") to satoshis.
fn coin_amount_to_satoshis(s: &str) -> i64 {
    const SATOSHI_PER_COIN: f64 = 100_000_000.0;
    s.trim()
        .parse::<f64>()
        .map(|v| (v * SATOSHI_PER_COIN).round() as i64)
        .unwrap_or(0)
}

/// Parse task::enable_utxo::status result details into CoinStatusDetails.
/// KDF returns spendable/unspendable as decimal strings (e.g. "4269.37384458"), not satoshis.
pub fn parse_status_details(details: &Value, ticker: &str) -> Option<CoinStatusDetails> {
    let current_block = details.get("current_block").and_then(|v| v.as_u64());
    let wb = details.get("wallet_balance")?;
    let wallet_type = wb.get("wallet_type").and_then(|v| v.as_str()).map(String::from);
    let address = wb.get("address").and_then(|v| v.as_str()).map(String::from);
    // Iguana: balance.TICKER.spendable, unspendable (decimal strings)
    if let Some(bal) = wb.get("balance") {
        let coin = bal.get(ticker)?;
        let spendable_str = coin.get("spendable").and_then(|v| v.as_str()).unwrap_or("0");
        let unspendable_str = coin.get("unspendable").and_then(|v| v.as_str()).unwrap_or("0");
        return Some(CoinStatusDetails {
            current_block,
            spendable_satoshis: coin_amount_to_satoshis(spendable_str),
            unspendable_satoshis: coin_amount_to_satoshis(unspendable_str),
            wallet_type,
            address,
        });
    }
    // HD: first account total_balance.TICKER
    let accounts = wb.get("accounts")?.as_array()?;
    let first = accounts.first()?;
    let total = first.get("total_balance")?.get(ticker)?;
    let spendable_str = total.get("spendable").and_then(|v| v.as_str()).unwrap_or("0");
    let unspendable_str = total.get("unspendable").and_then(|v| v.as_str()).unwrap_or("0");
    Some(CoinStatusDetails {
        current_block,
        spendable_satoshis: coin_amount_to_satoshis(spendable_str),
        unspendable_satoshis: coin_amount_to_satoshis(unspendable_str),
        wallet_type,
        address,
    })
}
