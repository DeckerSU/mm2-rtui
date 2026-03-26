# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**mm2-rtui** is a Rust TUI (terminal user interface) for the [Komodo DeFi Framework](https://github.com/KomodoPlatform/komodo-defi-framework) (KDF/MM2) — a cross-chain atomic-swap and P2P trading platform. The TUI manages wallet selection, coin activation, balance display, and transaction history by communicating with KDF via JSON-RPC.

## Commands

```bash
# Build release binary
cargo build --release
./target/release/mm2-rtui

# Build debug
cargo build

# Run tests
cargo test

# Run a specific test
cargo test <test_name>

# Check for compile errors without building
cargo check
```

The KDF binary must be present at the project root (named `kdf` or `mm2`, detected via process name). On startup, the app downloads required config files (`coins_config.json`, `coins.json`, `seed-nodes.json`) from GitHub if missing.

## Architecture

The app follows a single async event loop on the main Tokio thread, with background tasks for polling.

### Component Map

| File | Responsibility |
|------|---------------|
| `main.rs` | Startup sequence, terminal setup, main event loop, input handling |
| `app.rs` | Central `App` state struct, all TUI rendering via `ratatui` |
| `kdf_client.rs` | JSON-RPC client for KDF at `http://127.0.0.1:7783` |
| `coins.rs` | Coin types, activation parameters, satoshi conversion |
| `config.rs` | MM2.json generation, seed node parsing, RPC password management |
| `logger.rs` | In-memory circular log buffer (`SharedLogger = Arc<RwLock<Logger>>`) |
| `file_manager.rs` | Async download of coin config files from GitHub |
| `qr_compact.rs` | Compact QR code rendering using UTF-8 block characters |

### Startup Sequence

1. Create logger → download required files → generate/read MM2.json
2. Kill any existing KDF processes → spawn KDF binary (stdout/stderr → `kdf.log`)
3. Initialize terminal (raw mode, alternate screen, mouse capture)
4. Retry connection to KDF (5 attempts) → fetch wallet list → open wallet modal
5. Start background polling task (every 30s for version/block height)
6. Enter main event loop (polls every 16ms)

### Wallet → Coin Activation Flow

1. User selects wallet name and enters password in the modal
2. App kills KDF, updates MM2.json with wallet credentials, restarts KDF
3. On reconnect, calls `task::enable_utxo::init` for default coins (KMD, BTC)
4. Polls `task::enable_utxo::status` per task_id until `"Complete"`
5. Updates `App.coins` with spendable/unspendable balances

### State Management

- `App` struct in `app.rs` owns all UI state (coin list, modal state, log entries, pending tasks)
- Shared across async boundaries as `Arc<RwLock<App>>`
- `WalletModalState` enum drives the two-phase wallet UI (Selecting → EnteringPassword)
- Pending UTXO activations tracked by `task_id → ticker` map

### RPC Communication

All KDF calls use `mmrpc 2.0` format. All responses are written to `debug.log` with timestamps. The client is in `kdf_client.rs` and uses `reqwest` with `serde_json`.

### Key Bindings (main event loop)

- `Up`/`Down`: Navigate coin list
- `Enter`: Open coin details modal
- `I`: Show coin information
- `P`: Load next page of transaction history
- `H` (in wallet modal): Toggle HD wallet flag
- `Q` / `Esc`: Graceful shutdown (sends KDF `stop` RPC, waits for process exit)

## Generated/Runtime Files

These are not committed and are created at runtime:
- `MM2.json` — KDF runtime config with RPC password and wallet credentials
- `kdf.log` — KDF process stdout/stderr
- `debug.log` — All RPC request/response pairs with timestamps
- `coins_config.json`, `coins.json`, `seed-nodes.json` — Downloaded from GitHub on first run
