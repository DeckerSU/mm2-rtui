use chrono::{DateTime, Local};
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, HighlightSpacing, List, ListItem, ListState, Paragraph},
    Frame,
};
use crate::coins::Coin;
use crate::kdf_client;
use crate::logger::SharedLogger;
use std::sync::{Arc, Mutex};

/// Which screen is currently displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveScreen {
    Main,
    Swaps,
}

/// Which field is focused on the Swaps coin selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapsCoinFocus {
    Base,
    Rel,
    Orders,
}

/// Cached orderbook data.
#[derive(Debug, Clone)]
pub struct OrderbookData {
    pub asks: Vec<kdf_client::OrderbookEntry>,
    pub bids: Vec<kdf_client::OrderbookEntry>,
    pub base: String,
    pub rel: String,
    pub num_asks: u32,
    pub num_bids: u32,
    pub total_asks_base_vol: String,
    pub total_bids_base_vol: String,
}

/// State of the maker order creation modal.
#[derive(Debug, Clone)]
pub enum MakerOrderModal {
    /// Entering volume (amount of base coin to sell).
    EnteringVolume {
        base: String,
        rel: String,
        volume: String,
        /// Max spendable for base coin (display hint).
        max_base: String,
        /// Confirmation settings (read-only display).
        base_confs: u32,
        base_nota: bool,
        rel_confs: u32,
        rel_nota: bool,
    },
    /// Entering price (amount of rel per 1 base).
    EnteringPrice {
        base: String,
        rel: String,
        volume: String,
        price: String,
        max_base: String,
        base_confs: u32,
        base_nota: bool,
        rel_confs: u32,
        rel_nota: bool,
    },
    /// Confirming before sending.
    Confirming {
        base: String,
        rel: String,
        volume: String,
        price: String,
        base_confs: u32,
        base_nota: bool,
        rel_confs: u32,
        rel_nota: bool,
    },
    /// Waiting for RPC response.
    Sending,
    /// Show result (success or error).
    Result {
        success: bool,
        message: String,
    },
}

/// A user order entry for display in My Orders panel.
#[derive(Debug, Clone)]
pub struct MyOrderEntry {
    pub uuid: String,
    pub order_type: String,  // "Maker" or "Taker"
    pub base: String,
    pub rel: String,
    pub price: String,
    pub volume: String,
    pub cancellable: bool,
    pub status: String,      // placeholder for now
}

/// Order info modal content from order_status RPC.
#[derive(Debug, Clone)]
pub enum OrderInfoModal {
    Loading,
    MakerOrder {
        uuid: String,
        base: String,
        rel: String,
        price: String,
        max_base_vol: String,
        min_base_vol: String,
        available_amount: String,
        created_at: u64,
        updated_at: Option<u64>,
        cancellable: bool,
        cancellation_reason: Option<String>,
        started_swaps: Vec<String>,
        conf_settings: Option<(u32, bool, u32, bool)>,
    },
    TakerOrder {
        uuid: String,
        base: String,
        rel: String,
        base_amount: String,
        rel_amount: String,
        action: String,
        created_at: u64,
        cancellable: bool,
        order_type: Option<String>,
        cancellation_reason: Option<String>,
    },
    Error(String),
}

/// State of the wallet selection modal: either choosing a wallet or entering its password.
#[derive(Debug, Clone)]
pub enum WalletModalState {
    Selecting {
        names: Vec<String>,
        selected_index: usize,
        enable_hd: bool,
    },
    EnteringPassword {
        wallet_name: String,
        password: String,
        enable_hd: bool,
        /// Full list of wallet names; used to re-open selection if KDF restart fails.
        names: Vec<String>,
    },
}

/// State of the withdraw modal: entering address, entering amount, or confirming.
#[derive(Debug, Clone)]
pub enum WithdrawModalState {
    EnteringAddress {
        ticker: String,
        address: String,
    },
    EnteringAmount {
        ticker: String,
        address: String,
        amount: String,
    },
    Confirming {
        ticker: String,
        withdraw_result: crate::kdf_client::WithdrawResponse,
    },
    Sending,
    Result {
        success: bool,
        message: String,
    },
}

/// State for the coin activation selection modal.
#[derive(Debug, Clone)]
pub struct CoinSelectModal {
    /// All UTXO coins available for activation.
    pub all_coins: Vec<crate::coins::CoinEntry>,
    /// Filter text (starts-with match on ticker).
    pub filter: String,
    /// Currently highlighted index in the filtered list.
    pub selected_index: usize,
    /// Set of tickers selected for activation.
    pub selected_tickers: std::collections::HashSet<String>,
}

impl CoinSelectModal {
    pub fn new(all_coins: Vec<crate::coins::CoinEntry>) -> Self {
        Self {
            all_coins,
            filter: String::new(),
            selected_index: 0,
            selected_tickers: std::collections::HashSet::new(),
        }
    }

    /// Return filtered coin list based on the current filter.
    pub fn filtered(&self) -> Vec<&crate::coins::CoinEntry> {
        let f = self.filter.to_uppercase();
        self.all_coins
            .iter()
            .filter(|c| f.is_empty() || c.ticker.to_uppercase().starts_with(&f))
            .collect()
    }

    pub fn move_up(&mut self) {
        self.selected_index = self.selected_index.saturating_sub(1);
    }

    pub fn move_down(&mut self) {
        let max = self.filtered().len().saturating_sub(1);
        self.selected_index = (self.selected_index + 1).min(max);
    }

    /// Toggle selection of the currently highlighted coin.
    pub fn toggle_selected(&mut self) {
        let filtered = self.filtered();
        if let Some(entry) = filtered.get(self.selected_index) {
            let ticker = entry.ticker.clone();
            if !self.selected_tickers.remove(&ticker) {
                self.selected_tickers.insert(ticker);
            }
        }
    }

    pub fn push_filter_char(&mut self, c: char) {
        self.filter.push(c);
        self.selected_index = 0;
    }

    pub fn filter_backspace(&mut self) {
        self.filter.pop();
        self.selected_index = 0;
    }

    /// Return tickers selected for activation.
    pub fn get_selected_tickers(&self) -> Vec<String> {
        self.selected_tickers.iter().cloned().collect()
    }
}

/// Lines to scroll per PgUp/PgDn.
const LOG_PAGE_SIZE: usize = 8;
/// Visible log lines (log area height minus block borders).
const LOG_VISIBLE_LINES: usize = 6;

pub struct App {
    kdf_version: String,
    kdf_datetime: Option<DateTime<chrono::FixedOffset>>,
    current_time: DateTime<Local>,
    logger: SharedLogger,
    log_list_state: Arc<Mutex<ListState>>,
    /// When true, log list scrolls to bottom on each render (follow mode).
    log_follow: bool,
    /// Last key pressed (for status bar feedback).
    last_key_pressed: String,
    /// When true, show * in status bar instead of the actual key (e.g. during password entry).
    mask_keys: bool,
    /// Wallet selection modal: list of names and password input.
    wallet_modal: Option<WalletModalState>,
    /// Active coins and their balances (left panel).
    coins: Vec<Coin>,
    /// Pending UTXO activation task_ids and tickers.
    pending_tasks: Vec<(u64, String)>,
    /// Selection state for coins list (Up/Down, Enter for details).
    coins_list_state: Arc<Mutex<ListState>>,
    /// Information modal state (opened with I key).
    info_modal_open: bool,
    /// Transaction history for selected coin.
    tx_history: Vec<crate::kdf_client::Transaction>,
    /// Current page number for transaction history (1-indexed).
    tx_history_page: u32,
    /// Total pages available for transaction history.
    tx_history_total_pages: u32,
    /// Loading state for transaction history.
    tx_history_loading: bool,
    /// Error message for transaction history.
    tx_history_error: Option<String>,
    /// Withdraw modal state.
    withdraw_modal: Option<WithdrawModalState>,
    /// Coin activation selection modal.
    coin_select_modal: Option<CoinSelectModal>,
    /// Currently active screen.
    active_screen: ActiveScreen,
    /// Swaps screen: which coin selector is focused.
    swaps_focus: SwapsCoinFocus,
    /// Swaps screen: index of base coin in activated coins list.
    swaps_base_index: usize,
    /// Swaps screen: index of rel coin in activated coins list.
    swaps_rel_index: usize,
    /// Cached orderbook data.
    orderbook: Option<OrderbookData>,
    /// Orderbook loading state.
    orderbook_loading: bool,
    /// Orderbook error message.
    orderbook_error: Option<String>,
    /// Maker order creation modal.
    maker_order_modal: Option<MakerOrderModal>,
    /// My orders list (maker + taker).
    my_orders: Vec<MyOrderEntry>,
    /// Selected index in my_orders list (when focus is Orders).
    orders_selected_index: usize,
    /// Order info modal content (from order_status RPC).
    order_info_modal: Option<OrderInfoModal>,
}

impl App {
    pub fn new(logger: SharedLogger) -> Self {
        Self {
            kdf_version: "Unknown".to_string(),
            kdf_datetime: None,
            current_time: Local::now(),
            logger,
            log_list_state: Arc::new(Mutex::new(ListState::default())),
            log_follow: true,
            last_key_pressed: "—".to_string(),
            mask_keys: false,
            wallet_modal: None,
            coins: Vec::new(),
            pending_tasks: Vec::new(),
            coins_list_state: Arc::new(Mutex::new(ListState::default())),
            info_modal_open: false,
            tx_history: Vec::new(),
            tx_history_page: 1,
            tx_history_total_pages: 0,
            tx_history_loading: false,
            tx_history_error: None,
            withdraw_modal: None,
            coin_select_modal: None,
            active_screen: ActiveScreen::Main,
            swaps_focus: SwapsCoinFocus::Base,
            swaps_base_index: 0,
            swaps_rel_index: 1,
            orderbook: None,
            orderbook_loading: false,
            orderbook_error: None,
            maker_order_modal: None,
            my_orders: Vec::new(),
            orders_selected_index: 0,
            order_info_modal: None,
        }
    }

    pub fn add_coin(&mut self, coin: Coin) {
        if !self.coins.iter().any(|c| c.ticker == coin.ticker) {
            self.coins.push(coin);
        }
    }

    pub fn add_pending_task(&mut self, task_id: u64, ticker: String) {
        self.pending_tasks.push((task_id, ticker));
    }

    pub fn remove_pending_task(&mut self, task_id: u64) -> Option<String> {
        if let Some(pos) = self.pending_tasks.iter().position(|(id, _)| *id == task_id) {
            Some(self.pending_tasks.remove(pos).1)
        } else {
            None
        }
    }

    pub fn pending_tasks(&self) -> &[(u64, String)] {
        &self.pending_tasks[..]
    }

    pub fn update_coin_activated(&mut self, ticker: &str) {
        if let Some(c) = self.coins.iter_mut().find(|c| c.ticker == ticker) {
            c.activated = true;
        }
    }

    pub fn update_coin_from_status_details(
        &mut self,
        ticker: &str,
        details: &crate::coins::CoinStatusDetails,
    ) {
        if let Some(c) = self.coins.iter_mut().find(|c| c.ticker == ticker) {
            c.spendable_satoshis = Some(details.spendable_satoshis);
            c.unspendable_satoshis = Some(details.unspendable_satoshis);
            c.current_block = details.current_block;
            c.wallet_type = details.wallet_type.clone();
            c.address = details.address.clone();
        }
    }

    /// Update coin from my_balance response (spendable, unspendable in satoshis, address).
    pub fn update_coin_from_my_balance(
        &mut self,
        ticker: &str,
        spendable_satoshis: i64,
        unspendable_satoshis: i64,
        address: String,
    ) {
        if let Some(c) = self.coins.iter_mut().find(|c| c.ticker == ticker) {
            c.spendable_satoshis = Some(spendable_satoshis);
            c.unspendable_satoshis = Some(unspendable_satoshis);
            c.address = Some(address);
        }
    }

    pub fn coins_select_up(&mut self) {
        let len = self.coins.len();
        if len == 0 {
            return;
        }
        let mut state = self.coins_list_state.lock().unwrap();
        let current = state.selected().unwrap_or(0);
        let next = current.saturating_sub(1).min(len.saturating_sub(1));
        state.select(Some(next));
    }

    pub fn coins_select_down(&mut self) {
        let len = self.coins.len();
        if len == 0 {
            return;
        }
        let mut state = self.coins_list_state.lock().unwrap();
        let current = state.selected().unwrap_or(0);
        let next = (current + 1).min(len.saturating_sub(1));
        state.select(Some(next));
    }

    pub fn coins_selected_index(&self) -> Option<usize> {
        self.coins_list_state.lock().unwrap().selected()
    }

    /// Returns the ticker of the currently selected coin, if any.
    pub fn selected_coin_ticker(&self) -> Option<String> {
        self.coins_selected_index()
            .and_then(|i| self.coins.get(i))
            .map(|c| c.ticker.clone())
    }

    /// Open wallet selection modal with the list of wallet names from KDF.
    pub fn open_wallet_modal(&mut self, names: Vec<String>) {
        self.wallet_modal = Some(WalletModalState::Selecting {
            names,
            selected_index: 0,
            enable_hd: false,
        });
    }

    pub fn wallet_modal(&self) -> Option<&WalletModalState> {
        self.wallet_modal.as_ref()
    }

    pub fn wallet_modal_select_up(&mut self) {
        if let Some(WalletModalState::Selecting { names, selected_index, .. }) =
            &mut self.wallet_modal
        {
            *selected_index = selected_index.saturating_sub(1);
            if names.len() > 0 && *selected_index >= names.len() {
                *selected_index = names.len().saturating_sub(1);
            }
        }
    }

    pub fn wallet_modal_select_down(&mut self) {
        if let Some(WalletModalState::Selecting { names, selected_index, .. }) =
            &mut self.wallet_modal
        {
            *selected_index = (*selected_index + 1).min(names.len().saturating_sub(1));
        }
    }

    /// Toggle HD Wallet checkbox (e.g. with H key). Only in Selecting phase.
    pub fn wallet_modal_toggle_hd(&mut self) {
        if let Some(WalletModalState::Selecting { enable_hd, .. }) = &mut self.wallet_modal {
            *enable_hd = !*enable_hd;
        }
    }

    /// Confirm wallet selection (Enter in list). Switch to password input.
    pub fn wallet_modal_confirm_selection(&mut self) -> bool {
        if let Some(WalletModalState::Selecting {
            names,
            selected_index,
            enable_hd,
        }) = self.wallet_modal.take()
        {
            if let Some(name) = names.get(selected_index).cloned() {
                self.wallet_modal = Some(WalletModalState::EnteringPassword {
                    wallet_name: name,
                    password: String::new(),
                    enable_hd,
                    names: names.clone(),
                });
                return true;
            }
        }
        false
    }

    pub fn wallet_modal_password_push(&mut self, c: char) {
        if let Some(WalletModalState::EnteringPassword { password, .. }) = &mut self.wallet_modal
        {
            password.push(c);
        }
    }

    pub fn wallet_modal_password_backspace(&mut self) {
        if let Some(WalletModalState::EnteringPassword { password, .. }) = &mut self.wallet_modal
        {
            password.pop();
        }
    }

    /// Submit password (Enter). Returns (wallet_name, password, enable_hd, names) for restart and possible re-open on failure.
    pub fn wallet_modal_submit_password(&mut self) -> Option<(String, String, bool, Vec<String>)> {
        if let Some(WalletModalState::EnteringPassword {
            wallet_name,
            password,
            enable_hd,
            names,
        }) = self.wallet_modal.take()
        {
            return Some((wallet_name, password, enable_hd, names));
        }
        None
    }

    pub fn wallet_modal_close(&mut self) {
        self.wallet_modal = None;
    }

    pub fn set_last_key(&mut self, key: String) {
        self.last_key_pressed = if self.mask_keys { "*".to_string() } else { key };
    }

    pub fn set_mask_keys(&mut self, mask: bool) {
        self.mask_keys = mask;
    }

    /// Scroll log up by one page (PgUp).
    pub fn scroll_log_up(&mut self, entry_count: usize) {
        if entry_count == 0 {
            return;
        }
        self.log_follow = false;
        let mut state = self.log_list_state.lock().unwrap();
        let current = state.selected().unwrap_or(entry_count.saturating_sub(1));
        let new_idx = current.saturating_sub(LOG_PAGE_SIZE).min(entry_count.saturating_sub(1));
        state.select(Some(new_idx));
        // select() resets offset to 0; set offset so the selected item is visible
        *state.offset_mut() = new_idx.saturating_sub(LOG_VISIBLE_LINES.saturating_sub(1));
    }

    /// Scroll log down by one page (PgDn).
    pub fn scroll_log_down(&mut self, entry_count: usize) {
        if entry_count == 0 {
            return;
        }
        let mut state = self.log_list_state.lock().unwrap();
        let current = state.selected().unwrap_or(0);
        let new_idx = (current + LOG_PAGE_SIZE).min(entry_count.saturating_sub(1));
        state.select(Some(new_idx));
        // select() resets offset to 0; set offset so the selected item is visible
        *state.offset_mut() = new_idx.saturating_sub(LOG_VISIBLE_LINES.saturating_sub(1));
        self.log_follow = new_idx == entry_count.saturating_sub(1);
    }
    
    pub fn update_version(&mut self, version: String) {
        self.kdf_version = version;
    }
    
    pub fn update_datetime(&mut self, datetime: String) {
        if let Ok(dt) = DateTime::parse_from_rfc3339(&datetime) {
            self.kdf_datetime = Some(dt);
        }
    }
    
    pub fn update_current_time(&mut self) {
        self.current_time = Local::now();
    }
    
    /// Open information modal for the currently selected coin.
    pub fn open_info_modal(&mut self) {
        self.info_modal_open = true;
    }
    
    /// Close information modal.
    pub fn close_info_modal(&mut self) {
        self.info_modal_open = false;
    }
    
    /// Check if information modal is open.
    pub fn is_info_modal_open(&self) -> bool {
        self.info_modal_open
    }
    
    /// Set transaction history loading state.
    pub fn set_tx_history_loading(&mut self, loading: bool) {
        self.tx_history_loading = loading;
        if loading {
            self.tx_history_error = None;
        }
    }
    
    /// Update transaction history from API response.
    pub fn update_tx_history(
        &mut self,
        transactions: Vec<crate::kdf_client::Transaction>,
        page: u32,
        total_pages: u32,
        current_block: u64,
    ) {
        self.tx_history = transactions;
        self.tx_history_page = page;
        self.tx_history_total_pages = total_pages;
        self.tx_history_loading = false;
        self.tx_history_error = None;
        
        // Update current_block for the selected coin
        if let Some(idx) = self.coins_selected_index() {
            if idx < self.coins.len() {
                self.coins[idx].current_block = Some(current_block);
            }
        }
    }
    
    /// Set transaction history error.
    pub fn set_tx_history_error(&mut self, error: String) {
        self.tx_history_error = Some(error);
        self.tx_history_loading = false;
        self.tx_history = Vec::new();
    }
    
    /// Clear transaction history (when coin selection changes).
    pub fn clear_tx_history(&mut self) {
        self.tx_history.clear();
        self.tx_history_page = 1;
        self.tx_history_total_pages = 0;
        self.tx_history_loading = false;
        self.tx_history_error = None;
    }
    
    /// Get current transaction history page.
    pub fn tx_history_page(&self) -> u32 {
        self.tx_history_page
    }
    
    /// Get total transaction history pages.
    pub fn tx_history_total_pages(&self) -> u32 {
        self.tx_history_total_pages
    }
    
    /// Go to next page of transaction history.
    pub fn tx_history_next_page(&mut self) {
        if self.tx_history_page < self.tx_history_total_pages {
            self.tx_history_page += 1;
        }
    }
    
    /// Go to previous page of transaction history.
    pub fn tx_history_prev_page(&mut self) {
        if self.tx_history_page > 1 {
            self.tx_history_page -= 1;
        }
    }
    
    // --- Withdraw modal methods ---

    pub fn open_withdraw_modal(&mut self, ticker: String) {
        self.withdraw_modal = Some(WithdrawModalState::EnteringAddress {
            ticker,
            address: String::new(),
        });
    }

    pub fn withdraw_modal(&self) -> Option<&WithdrawModalState> {
        self.withdraw_modal.as_ref()
    }

    pub fn withdraw_modal_push_char(&mut self, c: char) {
        match &mut self.withdraw_modal {
            Some(WithdrawModalState::EnteringAddress { address, .. }) => {
                address.push(c);
            }
            Some(WithdrawModalState::EnteringAmount { amount, .. }) => {
                // Only allow digits and one dot
                if c.is_ascii_digit() || (c == '.' && !amount.contains('.')) {
                    amount.push(c);
                }
            }
            _ => {}
        }
    }

    pub fn withdraw_modal_backspace(&mut self) {
        match &mut self.withdraw_modal {
            Some(WithdrawModalState::EnteringAddress { address, .. }) => {
                address.pop();
            }
            Some(WithdrawModalState::EnteringAmount { amount, .. }) => {
                amount.pop();
            }
            _ => {}
        }
    }

    /// Confirm address entry → move to amount entry. Returns true if transitioned.
    pub fn withdraw_modal_confirm_address(&mut self) -> bool {
        if let Some(WithdrawModalState::EnteringAddress { ticker, address }) = self.withdraw_modal.take() {
            if !address.is_empty() {
                self.withdraw_modal = Some(WithdrawModalState::EnteringAmount {
                    ticker,
                    address,
                    amount: String::new(),
                });
                return true;
            }
            // Put it back if address is empty
            self.withdraw_modal = Some(WithdrawModalState::EnteringAddress { ticker, address });
        }
        false
    }

    /// Confirm amount entry → returns (ticker, address, amount) for withdraw RPC call.
    pub fn withdraw_modal_confirm_amount(&mut self) -> Option<(String, String, String)> {
        if let Some(WithdrawModalState::EnteringAmount { ticker, address, amount }) = self.withdraw_modal.take() {
            if !amount.is_empty() {
                // Keep modal in a "sending" state while we wait for withdraw RPC
                self.withdraw_modal = Some(WithdrawModalState::Sending);
                return Some((ticker, address, amount));
            }
            self.withdraw_modal = Some(WithdrawModalState::EnteringAmount { ticker, address, amount });
        }
        None
    }

    /// Set the withdraw confirmation state with the withdraw result.
    pub fn withdraw_modal_set_confirmation(&mut self, ticker: String, result: crate::kdf_client::WithdrawResponse) {
        self.withdraw_modal = Some(WithdrawModalState::Confirming {
            ticker,
            withdraw_result: result,
        });
    }

    /// User confirmed send → returns (ticker, coin, tx_hex) for send_raw_transaction.
    pub fn withdraw_modal_confirm_send(&mut self) -> Option<(String, String, String)> {
        if let Some(WithdrawModalState::Confirming { ticker, withdraw_result }) = self.withdraw_modal.take() {
            self.withdraw_modal = Some(WithdrawModalState::Sending);
            return Some((ticker, withdraw_result.coin, withdraw_result.tx_hex));
        }
        None
    }

    /// Set the final result of the withdraw (success or error).
    pub fn withdraw_modal_set_result(&mut self, success: bool, message: String) {
        self.withdraw_modal = Some(WithdrawModalState::Result { success, message });
    }

    /// Set error during withdraw preparation.
    pub fn withdraw_modal_set_error(&mut self, message: String) {
        self.withdraw_modal = Some(WithdrawModalState::Result { success: false, message });
    }

    pub fn close_withdraw_modal(&mut self) {
        self.withdraw_modal = None;
    }

    // --- Coin select modal methods ---

    pub fn open_coin_select_modal(&mut self, coins: Vec<crate::coins::CoinEntry>) {
        // Filter out already-active coins
        let active_tickers: std::collections::HashSet<&str> =
            self.coins.iter().map(|c| c.ticker.as_str()).collect();
        let available: Vec<_> = coins
            .into_iter()
            .filter(|c| !active_tickers.contains(c.ticker.as_str()))
            .collect();
        self.coin_select_modal = Some(CoinSelectModal::new(available));
    }

    pub fn coin_select_modal(&self) -> Option<&CoinSelectModal> {
        self.coin_select_modal.as_ref()
    }

    pub fn coin_select_modal_mut(&mut self) -> Option<&mut CoinSelectModal> {
        self.coin_select_modal.as_mut()
    }

    /// Close modal and return selected tickers (if Enter pressed).
    pub fn coin_select_modal_confirm(&mut self) -> Vec<String> {
        if let Some(modal) = self.coin_select_modal.take() {
            return modal.get_selected_tickers();
        }
        Vec::new()
    }

    pub fn close_coin_select_modal(&mut self) {
        self.coin_select_modal = None;
    }

    // --- Screen toggle ---

    pub fn active_screen(&self) -> ActiveScreen {
        self.active_screen
    }

    pub fn toggle_screen(&mut self) {
        self.active_screen = match self.active_screen {
            ActiveScreen::Main => ActiveScreen::Swaps,
            ActiveScreen::Swaps => ActiveScreen::Main,
        };
    }

    // --- Swaps screen methods ---

    pub fn swaps_focus(&self) -> SwapsCoinFocus {
        self.swaps_focus
    }

    pub fn swaps_toggle_focus(&mut self) {
        self.swaps_focus = match self.swaps_focus {
            SwapsCoinFocus::Base => SwapsCoinFocus::Rel,
            SwapsCoinFocus::Rel | SwapsCoinFocus::Orders => SwapsCoinFocus::Base,
        };
    }

    /// Returns tickers of activated coins.
    pub fn activated_coin_tickers(&self) -> Vec<String> {
        self.coins
            .iter()
            .filter(|c| c.activated)
            .map(|c| c.ticker.clone())
            .collect()
    }

    pub fn swaps_select_up(&mut self) {
        let activated = self.activated_coin_tickers();
        if activated.is_empty() {
            return;
        }
        match self.swaps_focus {
            SwapsCoinFocus::Base => {
                if self.swaps_base_index > 0 {
                    self.swaps_base_index -= 1;
                } else {
                    self.swaps_base_index = activated.len() - 1;
                }
            }
            SwapsCoinFocus::Rel => {
                if self.swaps_rel_index > 0 {
                    self.swaps_rel_index -= 1;
                } else {
                    self.swaps_rel_index = activated.len() - 1;
                }
            }
            SwapsCoinFocus::Orders => {
                if self.my_orders.is_empty() { return; }
                if self.orders_selected_index > 0 {
                    self.orders_selected_index -= 1;
                } else {
                    self.orders_selected_index = self.my_orders.len() - 1;
                }
            }
        }
    }

    pub fn swaps_select_down(&mut self) {
        let activated = self.activated_coin_tickers();
        if activated.is_empty() {
            return;
        }
        match self.swaps_focus {
            SwapsCoinFocus::Base => {
                self.swaps_base_index = (self.swaps_base_index + 1) % activated.len();
            }
            SwapsCoinFocus::Rel => {
                self.swaps_rel_index = (self.swaps_rel_index + 1) % activated.len();
            }
            SwapsCoinFocus::Orders => {
                if self.my_orders.is_empty() { return; }
                self.orders_selected_index = (self.orders_selected_index + 1) % self.my_orders.len();
            }
        }
    }

    /// Swap base and rel coin indices.
    pub fn swaps_flip_pair(&mut self) {
        std::mem::swap(&mut self.swaps_base_index, &mut self.swaps_rel_index);
    }

    /// Returns (base_ticker, rel_ticker) if both are valid and different.
    pub fn swaps_selected_pair(&self) -> Option<(String, String)> {
        let activated = self.activated_coin_tickers();
        if activated.len() < 2 {
            return None;
        }
        let base_idx = self.swaps_base_index % activated.len();
        let rel_idx = self.swaps_rel_index % activated.len();
        let base = &activated[base_idx];
        let rel = &activated[rel_idx];
        if base == rel {
            return None;
        }
        Some((base.clone(), rel.clone()))
    }

    pub fn swaps_base_ticker(&self) -> Option<String> {
        let activated = self.activated_coin_tickers();
        if activated.is_empty() {
            return None;
        }
        Some(activated[self.swaps_base_index % activated.len()].clone())
    }

    pub fn swaps_rel_ticker(&self) -> Option<String> {
        let activated = self.activated_coin_tickers();
        if activated.is_empty() {
            return None;
        }
        Some(activated[self.swaps_rel_index % activated.len()].clone())
    }

    pub fn set_orderbook(&mut self, data: OrderbookData) {
        self.orderbook = Some(data);
        self.orderbook_loading = false;
        self.orderbook_error = None;
    }

    pub fn set_orderbook_loading(&mut self, loading: bool) {
        self.orderbook_loading = loading;
        if loading {
            self.orderbook_error = None;
        }
    }

    pub fn set_orderbook_error(&mut self, error: String) {
        self.orderbook_error = Some(error);
        self.orderbook_loading = false;
    }

    pub fn orderbook(&self) -> Option<&OrderbookData> {
        self.orderbook.as_ref()
    }

    // --- Maker order modal ---

    pub fn maker_order_modal(&self) -> Option<&MakerOrderModal> {
        self.maker_order_modal.as_ref()
    }

    /// Open maker order modal for the currently selected base/rel pair.
    pub fn open_maker_order_modal(&mut self) {
        let base_ticker = match self.swaps_base_ticker() {
            Some(t) => t,
            None => return,
        };
        let rel_ticker = match self.swaps_rel_ticker() {
            Some(t) => t,
            None => return,
        };
        if base_ticker == rel_ticker {
            return;
        }
        // Get max spendable for base coin
        let max_base = self.coins.iter()
            .find(|c| c.ticker == base_ticker)
            .map(|c| c.spendable_display())
            .unwrap_or_else(|| "0".to_string());
        // Get confirmation settings
        let (base_confs, base_nota) = self.coins.iter()
            .find(|c| c.ticker == base_ticker)
            .map(|c| (c.required_confirmations, c.requires_notarization))
            .unwrap_or((2, false));
        let (rel_confs, rel_nota) = self.coins.iter()
            .find(|c| c.ticker == rel_ticker)
            .map(|c| (c.required_confirmations, c.requires_notarization))
            .unwrap_or((2, false));

        self.maker_order_modal = Some(MakerOrderModal::EnteringVolume {
            base: base_ticker,
            rel: rel_ticker,
            volume: String::new(),
            max_base,
            base_confs,
            base_nota,
            rel_confs,
            rel_nota,
        });
    }

    pub fn maker_order_modal_push_char(&mut self, c: char) {
        match &mut self.maker_order_modal {
            Some(MakerOrderModal::EnteringVolume { volume, .. }) => {
                if c.is_ascii_digit() || (c == '.' && !volume.contains('.')) {
                    volume.push(c);
                }
            }
            Some(MakerOrderModal::EnteringPrice { price, .. }) => {
                if c.is_ascii_digit() || (c == '.' && !price.contains('.')) {
                    price.push(c);
                }
            }
            _ => {}
        }
    }

    pub fn maker_order_modal_backspace(&mut self) {
        match &mut self.maker_order_modal {
            Some(MakerOrderModal::EnteringVolume { volume, .. }) => { volume.pop(); }
            Some(MakerOrderModal::EnteringPrice { price, .. }) => { price.pop(); }
            _ => {}
        }
    }

    /// Confirm volume → move to price entry. Returns true if transitioned.
    pub fn maker_order_modal_confirm_volume(&mut self) -> bool {
        if let Some(MakerOrderModal::EnteringVolume {
            base, rel, volume, max_base,
            base_confs, base_nota, rel_confs, rel_nota,
        }) = self.maker_order_modal.take()
        {
            if !volume.is_empty() && volume.parse::<f64>().unwrap_or(0.0) > 0.0 {
                self.maker_order_modal = Some(MakerOrderModal::EnteringPrice {
                    base, rel, volume, price: String::new(), max_base,
                    base_confs, base_nota, rel_confs, rel_nota,
                });
                return true;
            }
            self.maker_order_modal = Some(MakerOrderModal::EnteringVolume {
                base, rel, volume, max_base,
                base_confs, base_nota, rel_confs, rel_nota,
            });
        }
        false
    }

    /// Confirm price → move to confirmation. Returns true if transitioned.
    pub fn maker_order_modal_confirm_price(&mut self) -> bool {
        if let Some(MakerOrderModal::EnteringPrice {
            base, rel, volume, price, max_base: _,
            base_confs, base_nota, rel_confs, rel_nota,
        }) = self.maker_order_modal.take()
        {
            if !price.is_empty() && price.parse::<f64>().unwrap_or(0.0) > 0.0 {
                self.maker_order_modal = Some(MakerOrderModal::Confirming {
                    base, rel, volume, price,
                    base_confs, base_nota, rel_confs, rel_nota,
                });
                return true;
            }
            self.maker_order_modal = Some(MakerOrderModal::EnteringPrice {
                base, rel, volume, price, max_base: "".to_string(),
                base_confs, base_nota, rel_confs, rel_nota,
            });
        }
        false
    }

    /// User confirmed → returns (base, rel, volume, price, base_confs, base_nota, rel_confs, rel_nota).
    pub fn maker_order_modal_confirm_send(&mut self) -> Option<(String, String, String, String, u32, bool, u32, bool)> {
        if let Some(MakerOrderModal::Confirming {
            base, rel, volume, price,
            base_confs, base_nota, rel_confs, rel_nota,
        }) = self.maker_order_modal.take()
        {
            self.maker_order_modal = Some(MakerOrderModal::Sending);
            return Some((base, rel, volume, price, base_confs, base_nota, rel_confs, rel_nota));
        }
        None
    }

    pub fn maker_order_modal_set_result(&mut self, success: bool, message: String) {
        self.maker_order_modal = Some(MakerOrderModal::Result { success, message });
    }

    pub fn close_maker_order_modal(&mut self) {
        self.maker_order_modal = None;
    }

    // --- My orders ---

    pub fn update_my_orders(&mut self, orders: Vec<MyOrderEntry>) {
        self.my_orders = orders;
        if self.orders_selected_index >= self.my_orders.len() && !self.my_orders.is_empty() {
            self.orders_selected_index = self.my_orders.len() - 1;
        }
        if self.my_orders.is_empty() && self.swaps_focus == SwapsCoinFocus::Orders {
            self.swaps_focus = SwapsCoinFocus::Base;
        }
    }

    pub fn my_orders_list(&self) -> &[MyOrderEntry] {
        &self.my_orders
    }

    pub fn enter_orders_focus(&mut self) {
        if !self.my_orders.is_empty() {
            self.swaps_focus = SwapsCoinFocus::Orders;
            self.orders_selected_index = 0;
        }
    }

    pub fn exit_orders_focus(&mut self) {
        self.swaps_focus = SwapsCoinFocus::Base;
    }

    pub fn orders_selected_index(&self) -> usize {
        self.orders_selected_index
    }

    pub fn selected_order_uuid(&self) -> Option<String> {
        self.my_orders.get(self.orders_selected_index).map(|o| o.uuid.clone())
    }

    pub fn open_order_info_modal(&mut self, info: OrderInfoModal) {
        self.order_info_modal = Some(info);
    }

    pub fn close_order_info_modal(&mut self) {
        self.order_info_modal = None;
    }

    pub fn order_info_modal(&self) -> &Option<OrderInfoModal> {
        &self.order_info_modal
    }

    /// Format a decimal string to at most `max_decimals` decimal places, trimming trailing zeros.
    fn fmt_decimal(s: &str, max_decimals: usize) -> String {
        if let Some(dot_pos) = s.find('.') {
            let int_part = &s[..dot_pos];
            let frac_part = &s[dot_pos + 1..];
            let truncated = if frac_part.len() > max_decimals {
                &frac_part[..max_decimals]
            } else {
                frac_part
            };
            let trimmed = truncated.trim_end_matches('0');
            if trimmed.is_empty() {
                int_part.to_string()
            } else {
                format!("{}.{}", int_part, trimmed)
            }
        } else {
            s.to_string()
        }
    }

    fn render_swaps_screen(&self, f: &mut Frame, area: Rect) {
        let swaps_chunks = Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                Constraint::Length(3),    // coin selector bar
                Constraint::Percentage(60), // orderbook
                Constraint::Min(0),      // my orders placeholder
            ])
            .split(area);

        // --- Coin selector bar ---
        let selector_chunks = Layout::default()
            .direction(ratatui::layout::Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50),
                Constraint::Percentage(50),
            ])
            .split(swaps_chunks[0]);

        let base_ticker = self.swaps_base_ticker().unwrap_or_else(|| "—".to_string());
        let rel_ticker = self.swaps_rel_ticker().unwrap_or_else(|| "—".to_string());

        let base_style = if self.swaps_focus == SwapsCoinFocus::Base {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        let rel_style = if self.swaps_focus == SwapsCoinFocus::Rel {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };

        let base_para = Paragraph::new(format!(" Base: {} ", base_ticker))
            .block(Block::default().borders(Borders::ALL).title(" Base (↑/↓) "))
            .style(base_style);
        f.render_widget(base_para, selector_chunks[0]);

        let rel_para = Paragraph::new(format!(" Rel: {} ", rel_ticker))
            .block(Block::default().borders(Borders::ALL).title(" Rel (↑/↓) "))
            .style(rel_style);
        f.render_widget(rel_para, selector_chunks[1]);

        // --- Orderbook ---
        let ob_block = Block::default()
            .borders(Borders::ALL)
            .title(format!(
                " Orderbook: {}/{} (←/→ switch, Enter refresh, U flip) ",
                base_ticker, rel_ticker
            ));
        let ob_inner = ob_block.inner(swaps_chunks[1]);
        f.render_widget(ob_block, swaps_chunks[1]);

        if self.orderbook_loading {
            let loading = Paragraph::new("Loading orderbook...")
                .style(Style::default().fg(Color::Yellow));
            f.render_widget(loading, ob_inner);
        } else if let Some(ref error) = self.orderbook_error {
            let err_para = Paragraph::new(format!("Error: {}", error))
                .style(Style::default().fg(Color::Red));
            f.render_widget(err_para, ob_inner);
        } else if let Some(ref ob) = self.orderbook {
            // Split orderbook area into asks (top) and bids (bottom) with a spread line
            let ob_chunks = Layout::default()
                .direction(ratatui::layout::Direction::Vertical)
                .constraints([
                    Constraint::Length(1),     // header
                    Constraint::Percentage(50), // asks
                    Constraint::Length(1),      // spread separator
                    Constraint::Min(0),        // bids
                ])
                .split(ob_inner);

            // Header
            let header = Line::from(vec![
                Span::styled(
                    format!(" {:>12}  {:>14}  {:>14} ",
                        format!("Price({})", ob.rel),
                        format!("Amount({})", ob.base),
                        format!("Total({})", ob.rel),
                    ),
                    Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
                ),
            ]);
            f.render_widget(Paragraph::new(header), ob_chunks[0]);

            // Asks (sellers) — sorted ascending by price, displayed reversed so lowest ask is near spread
            let mut asks_sorted = ob.asks.clone();
            asks_sorted.sort_by(|a, b| {
                let pa: f64 = a.price.decimal.parse().unwrap_or(0.0);
                let pb: f64 = b.price.decimal.parse().unwrap_or(0.0);
                pa.partial_cmp(&pb).unwrap_or(std::cmp::Ordering::Equal)
            });
            // Show asks reversed (lowest near the spread line)
            asks_sorted.reverse();

            let ask_height = ob_chunks[1].height as usize;
            // Take only as many as fit, from the bottom of the sorted list (lowest prices)
            let visible_asks: Vec<_> = if asks_sorted.len() > ask_height {
                asks_sorted[asks_sorted.len() - ask_height..].to_vec()
            } else {
                asks_sorted
            };

            let mut ask_lines: Vec<Line> = Vec::new();
            // Pad with empty lines at top if fewer asks than available height
            for _ in 0..(ask_height.saturating_sub(visible_asks.len())) {
                ask_lines.push(Line::from(""));
            }
            for entry in &visible_asks {
                let price = Self::fmt_decimal(&entry.price.decimal, 8);
                let amount = Self::fmt_decimal(&entry.base_max_volume.decimal, 8);
                let total = Self::fmt_decimal(&entry.rel_max_volume.decimal, 8);
                ask_lines.push(Line::from(vec![
                    Span::styled(
                        format!(" {:>12}  {:>14}  {:>14} ", price, amount, total),
                        Style::default().fg(Color::Red),
                    ),
                ]));
            }
            f.render_widget(Paragraph::new(ask_lines), ob_chunks[1]);

            // Spread separator
            let best_ask: Option<f64> = ob.asks.iter()
                .filter_map(|e| e.price.decimal.parse::<f64>().ok())
                .reduce(f64::min);
            let best_bid: Option<f64> = ob.bids.iter()
                .filter_map(|e| e.price.decimal.parse::<f64>().ok())
                .reduce(f64::max);
            let spread_text = match (best_ask, best_bid) {
                (Some(a), Some(b)) => format!(
                    " ── Spread: {} ──",
                    Self::fmt_decimal(&format!("{:.8}", a - b), 8)
                ),
                _ => " ── Spread: — ──".to_string(),
            };
            let spread_line = Paragraph::new(spread_text)
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
                .alignment(Alignment::Center);
            f.render_widget(spread_line, ob_chunks[2]);

            // Bids (buyers) — sorted descending by price (highest bid at top)
            let mut bids_sorted = ob.bids.clone();
            bids_sorted.sort_by(|a, b| {
                let pa: f64 = a.price.decimal.parse().unwrap_or(0.0);
                let pb: f64 = b.price.decimal.parse().unwrap_or(0.0);
                pb.partial_cmp(&pa).unwrap_or(std::cmp::Ordering::Equal)
            });

            let bid_height = ob_chunks[3].height as usize;
            let visible_bids: Vec<_> = bids_sorted.into_iter().take(bid_height).collect();

            let mut bid_lines: Vec<Line> = Vec::new();
            for entry in &visible_bids {
                let price = Self::fmt_decimal(&entry.price.decimal, 8);
                let amount = Self::fmt_decimal(&entry.base_max_volume.decimal, 8);
                let total = Self::fmt_decimal(&entry.rel_max_volume.decimal, 8);
                bid_lines.push(Line::from(vec![
                    Span::styled(
                        format!(" {:>12}  {:>14}  {:>14} ", price, amount, total),
                        Style::default().fg(Color::Green),
                    ),
                ]));
            }
            f.render_widget(Paragraph::new(bid_lines), ob_chunks[3]);
        } else {
            // No orderbook loaded yet
            let hint = Paragraph::new("Select base/rel coins and press Enter to load orderbook")
                .style(Style::default().fg(Color::DarkGray));
            f.render_widget(hint, ob_inner);
        }

        // --- My Orders ---
        let orders_title = if self.swaps_focus == SwapsCoinFocus::Orders {
            " My Orders (Enter - info, Esc - back) "
        } else {
            " My Orders (I - select, M - new maker order) "
        };
        let orders_block = Block::default()
            .borders(Borders::ALL)
            .title(orders_title);
        let orders_inner = orders_block.inner(swaps_chunks[2]);
        f.render_widget(orders_block, swaps_chunks[2]);

        if self.my_orders.is_empty() {
            let hint = Paragraph::new("No active orders. Press M to create a maker order.")
                .style(Style::default().fg(Color::DarkGray));
            f.render_widget(hint, orders_inner);
        } else {
            let mut lines: Vec<Line> = Vec::new();
            // Header
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {:>6} {:>12} {:>10} {:>18} {:>18}  UUID",
                        "Type", "Pair", "Volume", "Price", "Total"
                    ),
                    Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
                ),
            ]));
            let in_orders_focus = self.swaps_focus == SwapsCoinFocus::Orders;
            for (i, order) in self.my_orders.iter().enumerate() {
                let vol_f = order.volume.parse::<f64>().unwrap_or(0.0);
                let price_f = order.price.parse::<f64>().unwrap_or(0.0);
                let total_f = vol_f * price_f;
                let vol = Self::fmt_decimal(&order.volume, 8);
                let price_fmt = format!("{:.8}", price_f);
                let total = format!("{:.8}", total_f);
                let pair = format!("{}→{}", order.base, order.rel);
                let type_color = if order.order_type == "Maker" { Color::Cyan } else { Color::Magenta };
                let is_selected = in_orders_focus && i == self.orders_selected_index;
                let bg = if is_selected { Color::DarkGray } else { Color::Reset };
                lines.push(Line::from(vec![
                    Span::styled(format!(" {:>6}", order.order_type), Style::default().fg(type_color).bg(bg)),
                    Span::styled(format!(" {:>12}", pair), Style::default().bg(bg)),
                    Span::styled(format!(" {:>10}", vol), Style::default().fg(Color::White).bg(bg)),
                    Span::styled(format!(" {:>18}", price_fmt), Style::default().fg(Color::Yellow).bg(bg)),
                    Span::styled(format!(" {:>18}", total), Style::default().fg(Color::Green).bg(bg)),
                    Span::styled(format!("  {}", order.uuid), Style::default().bg(bg)),
                ]));
            }
            let para = Paragraph::new(lines)
                .style(Style::default().fg(Color::White));
            f.render_widget(para, orders_inner);
        }
    }

    pub fn render(&self, f: &mut Frame) {
        // Split into: main area, log area, status bar
        let chunks = Layout::default()
            .constraints([
                Constraint::Min(0),
                Constraint::Length(8),
                Constraint::Length(3),
            ])
            .split(f.size());

        match self.active_screen {
            ActiveScreen::Main => self.render_main_screen(f, chunks[0]),
            ActiveScreen::Swaps => self.render_swaps_screen(f, chunks[0]),
        }
        // Log area
        self.render_log(f, chunks[1]);

        // Status bar
        self.render_status_bar(f, chunks[2]);

        // Modal overlays
        self.render_modals(f);
    }

    fn render_main_screen(&self, f: &mut Frame, area: Rect) {
        // Main area: left = coins + balances, right = Details + TX History
        let main_chunks = Layout::default()
            .direction(ratatui::layout::Direction::Horizontal)
            .constraints([Constraint::Length(28), Constraint::Min(0)])
            .split(area);

        // Split right panel into Details (top) and Transaction History (bottom)
        let right_chunks = Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([Constraint::Length(10), Constraint::Min(0)])
            .split(main_chunks[1]);

        let coins_block = Block::default()
            .borders(Borders::ALL)
            .title(" Coins (↑/↓ Enter, + activate) ");
        let coins_area = coins_block.inner(main_chunks[0]);
        f.render_widget(coins_block, main_chunks[0]);
        let list_width = coins_area.width as usize;
        let coin_items: Vec<ListItem> = self
            .coins
            .iter()
            .map(|c| {
                let ticker_part = format!("{}", c.ticker);
                let balance_part = c.balance_display();
                let pad_len = list_width
                    .saturating_sub(ticker_part.len())
                    .saturating_sub(balance_part.len())
                    .saturating_sub(2);
                let pad = " ".repeat(pad_len);
                let line = Line::from(vec![
                    Span::styled(ticker_part, Style::default().fg(Color::Cyan)),
                    Span::raw(pad),
                    Span::raw(balance_part),
                ]);
                ListItem::new(line)
            })
            .collect();
        if !coin_items.is_empty() {
            let list = List::new(coin_items)
                .highlight_style(
                    Style::default()
                        .bg(Color::DarkGray)
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("▸ ")
                .highlight_spacing(HighlightSpacing::Always);
            let mut list_state = self.coins_list_state.lock().unwrap().clone();
            let n = self.coins.len();
            if let Some(sel) = list_state.selected() {
                if sel >= n {
                    list_state.select(Some(n.saturating_sub(1)));
                }
            } else if n > 0 {
                list_state.select(Some(0));
            }
            f.render_stateful_widget(list, coins_area, &mut list_state);
            *self.coins_list_state.lock().unwrap() = list_state;
        }
        // Right panel top: Details for selected coin
        let details_block = Block::default()
            .borders(Borders::ALL)
            .title(" Details (R - refresh, I - info, W - withdraw) ");
        let details_area = details_block.inner(right_chunks[0]);
        f.render_widget(details_block, right_chunks[0]);
        let details_text = self
            .coins_selected_index()
            .and_then(|i| self.coins.get(i))
            .map(|c| {
                let mut lines = vec![
                    format!("ticker: {}", c.ticker),
                    format!(
                        "current_block: {}",
                        c.current_block
                            .map(|b| b.to_string())
                            .unwrap_or_else(|| "—".to_string())
                    ),
                    format!("spendable: {}", c.spendable_display()),
                    format!("unspendable: {}", c.unspendable_display()),
                ];
                if let Some(ref wt) = c.wallet_type {
                    lines.push(format!("wallet_type: {}", wt));
                }
                if let Some(ref addr) = c.address {
                    lines.push(format!("address: {}", addr));
                }
                lines.join("\n")
            })
            .unwrap_or_else(|| "Select a coin (↑/↓ Enter)".to_string());
        let details_para = Paragraph::new(details_text)
            .style(Style::default().fg(Color::Gray));
        f.render_widget(details_para, details_area);
        
        // Right panel bottom: Transaction History
        let tx_block = Block::default()
            .borders(Borders::ALL)
            .title(" Transaction History (N - next, P - prev) ");
        let tx_area = tx_block.inner(right_chunks[1]);
        f.render_widget(tx_block, right_chunks[1]);
        
        let mut tx_content = Vec::new();
        
        if self.tx_history_loading {
            tx_content.push(Line::from(vec![
                Span::styled("Loading transactions...", Style::default().fg(Color::Yellow)),
            ]));
        } else if let Some(ref error) = self.tx_history_error {
            tx_content.push(Line::from(vec![
                Span::styled(format!("Error: {}", error), Style::default().fg(Color::Red)),
            ]));
        } else if self.tx_history.is_empty() {
            tx_content.push(Line::from(vec![
                Span::styled("No transactions found", Style::default().fg(Color::DarkGray)),
            ]));
        } else {
            // Display transactions
            for tx in &self.tx_history {
                // Determine transaction type (SEND or RECEIVE)
                let tx_type = if tx.my_balance_change.starts_with('-') {
                    ("SEND", Color::Red)
                } else {
                    ("RECV", Color::Green)
                };
                
                // Check if transaction is unconfirmed
                // Unconfirmed if: timestamp=0 OR block_height=0 OR confirmations > current_block
                let is_unconfirmed = tx.timestamp == 0 || tx.block_height == 0;
                
                // Format timestamp
                let dt = if is_unconfirmed {
                    "NOW".to_string()
                } else {
                    chrono::DateTime::from_timestamp(tx.timestamp, 0)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_else(|| "Unknown".to_string())
                };
                
                // Format confirmations
                let conf_display = if is_unconfirmed {
                    "Unconfirmed".to_string()
                } else {
                    format!("{} conf", tx.confirmations)
                };
                
                let conf_color = if is_unconfirmed { Color::Red } else { Color::Yellow };
                
                // First line: [TYPE] Date Amount [Confirmations]
                tx_content.push(Line::from(vec![
                    Span::styled(format!("[{}] ", tx_type.0), Style::default().fg(tx_type.1).add_modifier(Modifier::BOLD)),
                    Span::raw(format!("{} ", dt)),
                    Span::styled(format!("{} {} ", tx.my_balance_change, tx.coin), Style::default().fg(Color::Cyan)),
                    Span::styled(format!("[{}]", conf_display), Style::default().fg(conf_color)),
                ]));
                
                // Second line: Hash (abbreviated)
                let hash_display = if tx.tx_hash.len() > 24 {
                    format!("{}...{}", &tx.tx_hash[..16], &tx.tx_hash[tx.tx_hash.len()-8..])
                } else {
                    tx.tx_hash.clone()
                };
                tx_content.push(Line::from(vec![
                    Span::styled("Hash: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(hash_display),
                ]));
                
                // Third line: From/To addresses (abbreviated)
                let from_display = if !tx.from.is_empty() {
                    let addr = &tx.from[0];
                    if addr.len() > 20 {
                        format!("{}...", &addr[..17])
                    } else {
                        addr.clone()
                    }
                } else {
                    "Unknown".to_string()
                };
                
                let to_display = if !tx.to.is_empty() {
                    let addr = &tx.to[0];
                    if addr.len() > 20 {
                        format!("{}...", &addr[..17])
                    } else {
                        addr.clone()
                    }
                } else {
                    "Unknown".to_string()
                };
                
                tx_content.push(Line::from(vec![
                    Span::styled("From: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(format!("{} ", from_display)),
                    Span::styled("To: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(to_display),
                ]));
                
                // Status line
                let status = if is_unconfirmed {
                    ("Unconfirmed", Color::Red)
                } else if tx.confirmations > 0 {
                    ("Confirmed", Color::Green)
                } else {
                    ("Pending", Color::Yellow)
                };
                tx_content.push(Line::from(vec![
                    Span::styled("Status: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(status.0, Style::default().fg(status.1)),
                ]));
                
                // Separator
                tx_content.push(Line::from("────────────────────────────────────────"));
            }
            
            // Page info at the end
            if self.tx_history_total_pages > 0 {
                tx_content.push(Line::from(""));
                tx_content.push(Line::from(vec![
                    Span::styled(
                        format!("Page: {}/{}", self.tx_history_page, self.tx_history_total_pages),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ),
                ]));
            }
        }
        
        let tx_para = Paragraph::new(tx_content)
            .style(Style::default().fg(Color::White));
        f.render_widget(tx_para, tx_area);
    }

    fn render_log(&self, f: &mut Frame, area: Rect) {
        if let Ok(logger) = self.logger.read() {
            let entries = logger.get_entries();
            let current_count = entries.len();
            
            let mut list_state = self.log_list_state.lock().unwrap().clone();
            if current_count > 0 {
                if self.log_follow {
                    list_state.select(Some(current_count.saturating_sub(1)));
                } else {
                    // Clamp selection to valid range
                    let selected = list_state.selected().unwrap_or(0);
                    let clamped = selected.min(current_count.saturating_sub(1));
                    list_state.select(Some(clamped));
                }
            }
            
            let items: Vec<ListItem> = entries
                .iter()
                .map(|entry| {
                    let level_str = entry.level.as_str();
                    let level_color = entry.level.color();
                    let line = Line::from(vec![
                        Span::styled(
                            format!("[{}] ", entry.timestamp),
                            Style::default().fg(Color::Gray),
                        ),
                        Span::styled(
                            format!("[{}] ", level_str),
                            Style::default().fg(level_color),
                        ),
                        Span::raw(&entry.message),
                    ]);
                    ListItem::new(line)
                })
                .collect();
            
            let log_list = List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Log"),
                );
            
            f.render_stateful_widget(log_list, area, &mut list_state);

            // Update state
            *self.log_list_state.lock().unwrap() = list_state;
        }
    }

    fn render_status_bar(&self, f: &mut Frame, area: Rect) {
        let screen_label = match self.active_screen {
            ActiveScreen::Main => "[Tab] Main | Swaps",
            ActiveScreen::Swaps => "Main | [Tab] Swaps",
        };

        let status_chunks = Layout::default()
            .direction(ratatui::layout::Direction::Horizontal)
            .constraints([
                Constraint::Percentage(40),
                Constraint::Percentage(20),
                Constraint::Percentage(40),
            ])
            .split(area);

        let version_text = format!("{} | KDF: {}", screen_label, self.kdf_version);
        let version_para = Paragraph::new(version_text)
            .block(Block::default().borders(Borders::ALL))
            .style(Style::default().fg(Color::Cyan))
            .alignment(Alignment::Left);
        f.render_widget(version_para, status_chunks[0]);
        
        let key_text = if self.last_key_pressed.is_empty() {
            "—".to_string()
        } else {
            self.last_key_pressed.clone()
        };
        let key_para = Paragraph::new(key_text)
            .block(Block::default().borders(Borders::ALL).title("Key"))
            .style(Style::default().fg(Color::Green))
            .alignment(Alignment::Center);
        f.render_widget(key_para, status_chunks[1]);
        
        let time_text = self.current_time.format("%Y-%m-%d %H:%M:%S").to_string();
        let time_para = Paragraph::new(time_text)
            .block(Block::default().borders(Borders::ALL))
            .style(Style::default().fg(Color::Yellow))
            .alignment(Alignment::Right);
        f.render_widget(time_para, status_chunks[2]);
    }

    fn render_modals(&self, f: &mut Frame) {
        // Wallet selection modal (centered overlay)
        if let Some(state) = &self.wallet_modal {
            let area = f.size();
            let modal_w = 50.min(area.width.saturating_sub(4));
            let modal_h = 18.min(area.height.saturating_sub(4));
            let x = area.width.saturating_sub(modal_w) / 2;
            let y = area.height.saturating_sub(modal_h) / 2;
            let modal_rect = Rect::new(x, y, modal_w, modal_h);

            let clear = Clear;
            f.render_widget(clear, modal_rect);
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" Select wallet ");
            let inner = block.inner(modal_rect);
            f.render_widget(block, modal_rect);

            match state {
                WalletModalState::Selecting {
                    names,
                    selected_index,
                    enable_hd,
                } => {
                    let chunks = Layout::default()
                        .constraints([
                            Constraint::Min(3),
                            Constraint::Length(1),
                            Constraint::Length(1),
                        ])
                        .split(inner);
                    let list_area = chunks[0];
                    let checkbox_area = chunks[1];
                    let hint_area = chunks[2];

                    let items: Vec<ListItem> = names
                        .iter()
                        .map(|n| ListItem::new(Line::from(n.as_str())))
                        .collect();
                    let visible_rows = list_area.height as usize;
                    let mut list_state = ListState::default();
                    list_state.select(Some(*selected_index));
                    if visible_rows > 0 && !names.is_empty() {
                        let offset = selected_index
                            .saturating_sub(visible_rows.saturating_sub(1))
                            .min(names.len().saturating_sub(1));
                        *list_state.offset_mut() = offset;
                    }
                    let list = List::new(items)
                        .highlight_style(
                            Style::default()
                                .bg(Color::Cyan)
                                .fg(Color::Black)
                                .add_modifier(Modifier::BOLD),
                        )
                        .highlight_symbol("▸ ")
                        .highlight_spacing(HighlightSpacing::Always);
                    f.render_stateful_widget(list, list_area, &mut list_state);

                    let hd_label = if *enable_hd {
                        "[x] HD Wallet"
                    } else {
                        "[ ] HD Wallet"
                    };
                    let checkbox_line = Line::from(vec![
                        Span::styled(
                            hd_label,
                            Style::default()
                                .fg(if *enable_hd { Color::Green } else { Color::DarkGray }),
                        ),
                    ]);
                    let checkbox_para = Paragraph::new(checkbox_line);
                    f.render_widget(checkbox_para, checkbox_area);

                    let hint = "↑/↓ choose  Enter  H: HD  Esc cancel";
                    let hint_para = Paragraph::new(hint)
                        .style(Style::default().fg(Color::DarkGray));
                    f.render_widget(hint_para, hint_area);
                }
                WalletModalState::EnteringPassword {
                    wallet_name,
                    password,
                    enable_hd: _,
                    names: _,
                } => {
                    let chunks = Layout::default()
                        .constraints([
                            Constraint::Length(2),
                            Constraint::Length(3),
                            Constraint::Min(0),
                        ])
                        .split(inner);
                    let title = Paragraph::new(format!("Wallet: {}", wallet_name))
                        .style(Style::default().fg(Color::Green));
                    f.render_widget(title, chunks[0]);
                    let prompt = Paragraph::new("Wallet Password:")
                        .style(Style::default().fg(Color::Gray));
                    f.render_widget(prompt, chunks[1]);
                    let masked = "*".repeat(password.chars().count());
                    let input = Paragraph::new(masked)
                        .block(Block::default().borders(Borders::ALL))
                        .style(Style::default().fg(Color::White));
                    f.render_widget(input, chunks[2]);
                }
            }
        }
        
        // Information modal (centered overlay) - shows address and QR code
        if self.info_modal_open {
            if let Some(coin) = self.coins_selected_index().and_then(|i| self.coins.get(i)) {
                let area = f.size();
                let modal_w = 70.min(area.width.saturating_sub(4));
                let modal_h = 30.min(area.height.saturating_sub(4));
                let x = area.width.saturating_sub(modal_w) / 2;
                let y = area.height.saturating_sub(modal_h) / 2;
                let modal_rect = Rect::new(x, y, modal_w, modal_h);

                let clear = Clear;
                f.render_widget(clear, modal_rect);
                let block = Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title(format!(" Coin Information - {} ", coin.ticker));
                let inner = block.inner(modal_rect);
                f.render_widget(block, modal_rect);

                let mut content = Vec::new();
                
                // Address section
                content.push(Line::from(vec![
                    Span::styled("Address:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                ]));
                
                if let Some(ref addr) = coin.address {
                    content.push(Line::from(addr.clone()));
                    content.push(Line::from(""));
                    
                    // QR Code section
                    content.push(Line::from(vec![
                        Span::styled("QR Code:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    ]));
                    content.push(Line::from(""));
                    
                    // Generate compact QR code using UTF-8 block characters
                    match crate::qr_compact::render_qr_compact(addr) {
                        Ok(qr_lines) => {
                            for line in qr_lines {
                                content.push(Line::from(line));
                            }
                        }
                        Err(_) => {
                            content.push(Line::from(vec![
                                Span::styled("Failed to generate QR code", Style::default().fg(Color::Red)),
                            ]));
                        }
                    }
                } else {
                    content.push(Line::from(vec![
                        Span::styled("Address not available for this coin", Style::default().fg(Color::Red)),
                    ]));
                }
                
                content.push(Line::from(""));
                content.push(Line::from(""));
                content.push(Line::from(vec![
                    Span::styled("Press Esc to close", Style::default().fg(Color::DarkGray)),
                ]));
                
                let para = Paragraph::new(content)
                    .style(Style::default().fg(Color::White));
                f.render_widget(para, inner);
            }
        }

        // Coin activation selection modal (centered overlay)
        if let Some(modal) = &self.coin_select_modal {
            let area = f.size();
            let modal_w = 60.min(area.width.saturating_sub(4));
            let modal_h = 30.min(area.height.saturating_sub(4));
            let x = area.width.saturating_sub(modal_w) / 2;
            let y = area.height.saturating_sub(modal_h) / 2;
            let modal_rect = Rect::new(x, y, modal_w, modal_h);

            f.render_widget(Clear, modal_rect);

            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" Activate Coins (Space=toggle, Enter=confirm, Esc=cancel) ");
            let inner = block.inner(modal_rect);
            f.render_widget(block, modal_rect);

            let chunks = Layout::default()
                .constraints([
                    Constraint::Length(3),
                    Constraint::Length(1),
                    Constraint::Min(1),
                ])
                .split(inner);

            // Filter input
            let filter_block = Block::default()
                .borders(Borders::ALL)
                .title(" Filter (starts with) ");
            let filter_text = Paragraph::new(modal.filter.as_str())
                .block(filter_block)
                .style(Style::default().fg(Color::White));
            f.render_widget(filter_text, chunks[0]);

            // Selected count
            let count_text = format!(
                " {} coin(s) selected",
                modal.selected_tickers.len()
            );
            let count_para = Paragraph::new(count_text)
                .style(Style::default().fg(Color::Yellow));
            f.render_widget(count_para, chunks[1]);

            // Coin list
            let filtered = modal.filtered();
            let items: Vec<ListItem> = filtered
                .iter()
                .enumerate()
                .map(|(i, entry)| {
                    let selected = modal.selected_tickers.contains(&entry.ticker);
                    let marker = if selected { "[x]" } else { "[ ]" };
                    let style = if i == modal.selected_index {
                        if selected {
                            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD | Modifier::REVERSED)
                        } else {
                            Style::default().add_modifier(Modifier::REVERSED)
                        }
                    } else if selected {
                        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    let text = format!("{} {:10} {}", marker, entry.ticker, entry.fname);
                    ListItem::new(text).style(style)
                })
                .collect();

            let mut list_state = ListState::default();
            if !filtered.is_empty() {
                list_state.select(Some(modal.selected_index));
            }

            let list = List::new(items)
                .block(Block::default().borders(Borders::NONE))
                .highlight_spacing(HighlightSpacing::Always);
            f.render_stateful_widget(list, chunks[2], &mut list_state);
        }

        // Withdraw modal (centered overlay)
        if let Some(wstate) = &self.withdraw_modal {
            let area = f.size();
            let modal_w = 70.min(area.width.saturating_sub(4));
            let modal_h = 20.min(area.height.saturating_sub(4));
            let x = area.width.saturating_sub(modal_w) / 2;
            let y = area.height.saturating_sub(modal_h) / 2;
            let modal_rect = Rect::new(x, y, modal_w, modal_h);

            f.render_widget(Clear, modal_rect);

            match wstate {
                WithdrawModalState::EnteringAddress { ticker, address } => {
                    let block = Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Yellow))
                        .title(format!(" Withdraw {} — Enter Address ", ticker));
                    let inner = block.inner(modal_rect);
                    f.render_widget(block, modal_rect);

                    let chunks = Layout::default()
                        .constraints([
                            Constraint::Length(2),
                            Constraint::Length(3),
                            Constraint::Min(0),
                        ])
                        .split(inner);

                    let hint = Paragraph::new("Enter the destination address:")
                        .style(Style::default().fg(Color::Gray));
                    f.render_widget(hint, chunks[0]);

                    let input = Paragraph::new(address.as_str())
                        .block(Block::default().borders(Borders::ALL))
                        .style(Style::default().fg(Color::White));
                    f.render_widget(input, chunks[1]);

                    let footer = Paragraph::new("Enter — next  |  Esc — cancel")
                        .style(Style::default().fg(Color::DarkGray));
                    f.render_widget(footer, chunks[2]);
                }
                WithdrawModalState::EnteringAmount { ticker, address, amount } => {
                    let block = Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Yellow))
                        .title(format!(" Withdraw {} — Enter Amount ", ticker));
                    let inner = block.inner(modal_rect);
                    f.render_widget(block, modal_rect);

                    let chunks = Layout::default()
                        .constraints([
                            Constraint::Length(2),
                            Constraint::Length(3),
                            Constraint::Min(0),
                        ])
                        .split(inner);

                    let addr_display = format!("To: {}", address);
                    let info = Paragraph::new(addr_display)
                        .style(Style::default().fg(Color::Cyan));
                    f.render_widget(info, chunks[0]);

                    let input = Paragraph::new(amount.as_str())
                        .block(Block::default().borders(Borders::ALL).title(" Amount "))
                        .style(Style::default().fg(Color::White));
                    f.render_widget(input, chunks[1]);

                    let footer = Paragraph::new("Enter — confirm  |  Esc — cancel")
                        .style(Style::default().fg(Color::DarkGray));
                    f.render_widget(footer, chunks[2]);
                }
                WithdrawModalState::Confirming { ticker, withdraw_result } => {
                    let block = Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Red))
                        .title(format!(" Confirm Withdraw {} ", ticker));
                    let inner = block.inner(modal_rect);
                    f.render_widget(block, modal_rect);

                    let to_addr = withdraw_result.to.first().cloned().unwrap_or_default();
                    let from_addr = withdraw_result.from.first().cloned().unwrap_or_default();

                    // Extract fee info
                    let fee_display = if let Some(obj) = withdraw_result.fee_details.as_object() {
                        if let Some(amount) = obj.get("amount") {
                            format!("{} {}", amount.as_str().unwrap_or(&amount.to_string()),
                                obj.get("coin").and_then(|c| c.as_str()).unwrap_or(ticker))
                        } else if let Some(total_fee) = obj.get("total_fee") {
                            format!("{} {}", total_fee.as_str().unwrap_or(&total_fee.to_string()),
                                obj.get("coin").and_then(|c| c.as_str()).unwrap_or(ticker))
                        } else {
                            withdraw_result.fee_details.to_string()
                        }
                    } else {
                        withdraw_result.fee_details.to_string()
                    };

                    let lines = vec![
                        Line::from(""),
                        Line::from(vec![
                            Span::styled("  From:   ", Style::default().fg(Color::DarkGray)),
                            Span::styled(&from_addr, Style::default().fg(Color::White)),
                        ]),
                        Line::from(vec![
                            Span::styled("  To:     ", Style::default().fg(Color::DarkGray)),
                            Span::styled(&to_addr, Style::default().fg(Color::Cyan)),
                        ]),
                        Line::from(vec![
                            Span::styled("  Amount: ", Style::default().fg(Color::DarkGray)),
                            Span::styled(
                                format!("{} {}", withdraw_result.total_amount, ticker),
                                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                            ),
                        ]),
                        Line::from(vec![
                            Span::styled("  Fee:    ", Style::default().fg(Color::DarkGray)),
                            Span::styled(&fee_display, Style::default().fg(Color::Yellow)),
                        ]),
                        Line::from(vec![
                            Span::styled("  Balance change: ", Style::default().fg(Color::DarkGray)),
                            Span::styled(&withdraw_result.my_balance_change, Style::default().fg(Color::Red)),
                        ]),
                        Line::from(""),
                        Line::from(""),
                        Line::from(vec![
                            Span::styled(
                                "  Y — SEND TRANSACTION  |  Esc — cancel",
                                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                            ),
                        ]),
                    ];

                    let para = Paragraph::new(lines)
                        .style(Style::default().fg(Color::White));
                    f.render_widget(para, inner);
                }
                WithdrawModalState::Sending => {
                    let block = Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Yellow))
                        .title(" Withdraw ");
                    let inner = block.inner(modal_rect);
                    f.render_widget(block, modal_rect);

                    let para = Paragraph::new("Processing...")
                        .style(Style::default().fg(Color::Yellow));
                    f.render_widget(para, inner);
                }
                WithdrawModalState::Result { success, message } => {
                    let color = if *success { Color::Green } else { Color::Red };
                    let title = if *success { " Withdraw Success " } else { " Withdraw Error " };
                    let block = Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(color))
                        .title(title);
                    let inner = block.inner(modal_rect);
                    f.render_widget(block, modal_rect);

                    let mut lines = vec![Line::from("")];
                    for part in message.split('\n') {
                        lines.push(Line::from(vec![
                            Span::styled(part.to_string(), Style::default().fg(color)),
                        ]));
                    }
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled("Press Esc to close", Style::default().fg(Color::DarkGray)),
                    ]));

                    let para = Paragraph::new(lines)
                        .style(Style::default().fg(Color::White));
                    f.render_widget(para, inner);
                }
            }
        }

        // Maker order modal (centered overlay)
        if let Some(mstate) = &self.maker_order_modal {
            let area = f.size();
            let modal_w = 70.min(area.width.saturating_sub(4));
            let modal_h = 22.min(area.height.saturating_sub(4));
            let x = area.width.saturating_sub(modal_w) / 2;
            let y = area.height.saturating_sub(modal_h) / 2;
            let modal_rect = Rect::new(x, y, modal_w, modal_h);

            f.render_widget(Clear, modal_rect);

            match mstate {
                MakerOrderModal::EnteringVolume {
                    base, rel, volume, max_base,
                    base_confs, base_nota, rel_confs, rel_nota,
                } => {
                    let block = Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan))
                        .title(format!(" Maker Order: {} → {} — Volume ", base, rel));
                    let inner = block.inner(modal_rect);
                    f.render_widget(block, modal_rect);

                    let chunks = Layout::default()
                        .constraints([
                            Constraint::Length(3),
                            Constraint::Length(3),
                            Constraint::Length(3),
                            Constraint::Min(0),
                        ])
                        .split(inner);

                    let conf_text = format!(
                        "  {} Confirmations: {} (dPoW: {})  |  {} Confirmations: {} (dPoW: {})",
                        base, base_confs, base_nota, rel, rel_confs, rel_nota
                    );
                    let conf_para = Paragraph::new(conf_text)
                        .style(Style::default().fg(Color::DarkGray));
                    f.render_widget(conf_para, chunks[0]);

                    let hint = format!("  Sell {} (max: {})", base, max_base);
                    let hint_para = Paragraph::new(hint)
                        .style(Style::default().fg(Color::Yellow));
                    f.render_widget(hint_para, chunks[1]);

                    let input = Paragraph::new(volume.as_str())
                        .block(Block::default().borders(Borders::ALL).title(format!(" Volume ({}) ", base)))
                        .style(Style::default().fg(Color::White));
                    f.render_widget(input, chunks[2]);

                    let footer = Paragraph::new("  Enter — next  |  Esc — cancel")
                        .style(Style::default().fg(Color::DarkGray));
                    f.render_widget(footer, chunks[3]);
                }
                MakerOrderModal::EnteringPrice {
                    base, rel, volume, price, max_base: _,
                    base_confs, base_nota, rel_confs, rel_nota,
                } => {
                    let block = Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan))
                        .title(format!(" Maker Order: {} → {} — Price ", base, rel));
                    let inner = block.inner(modal_rect);
                    f.render_widget(block, modal_rect);

                    let chunks = Layout::default()
                        .constraints([
                            Constraint::Length(3),
                            Constraint::Length(2),
                            Constraint::Length(3),
                            Constraint::Min(0),
                        ])
                        .split(inner);

                    let conf_text = format!(
                        "  {} Confirmations: {} (dPoW: {})  |  {} Confirmations: {} (dPoW: {})",
                        base, base_confs, base_nota, rel, rel_confs, rel_nota
                    );
                    let conf_para = Paragraph::new(conf_text)
                        .style(Style::default().fg(Color::DarkGray));
                    f.render_widget(conf_para, chunks[0]);

                    let info = format!("  Volume: {} {}", volume, base);
                    let info_para = Paragraph::new(info)
                        .style(Style::default().fg(Color::Cyan));
                    f.render_widget(info_para, chunks[1]);

                    let input = Paragraph::new(price.as_str())
                        .block(Block::default().borders(Borders::ALL).title(format!(" Price ({} per 1 {}) ", rel, base)))
                        .style(Style::default().fg(Color::White));
                    f.render_widget(input, chunks[2]);

                    let footer = Paragraph::new("  Enter — confirm  |  Esc — cancel")
                        .style(Style::default().fg(Color::DarkGray));
                    f.render_widget(footer, chunks[3]);
                }
                MakerOrderModal::Confirming {
                    base, rel, volume, price,
                    base_confs, base_nota, rel_confs, rel_nota,
                } => {
                    let block = Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Red))
                        .title(format!(" Confirm Maker Order: {} → {} ", base, rel));
                    let inner = block.inner(modal_rect);
                    f.render_widget(block, modal_rect);

                    let vol_f = volume.parse::<f64>().unwrap_or(0.0);
                    let price_f = price.parse::<f64>().unwrap_or(0.0);
                    let total = Self::fmt_decimal(&format!("{:.8}", vol_f * price_f), 8);

                    let lines = vec![
                        Line::from(""),
                        Line::from(vec![
                            Span::styled("  Sell:   ", Style::default().fg(Color::DarkGray)),
                            Span::styled(format!("{} {}", volume, base), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                        ]),
                        Line::from(vec![
                            Span::styled("  Price:  ", Style::default().fg(Color::DarkGray)),
                            Span::styled(format!("{} {} per 1 {}", price, rel, base), Style::default().fg(Color::Yellow)),
                        ]),
                        Line::from(vec![
                            Span::styled("  Get:    ", Style::default().fg(Color::DarkGray)),
                            Span::styled(format!("{} {}", total, rel), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                        ]),
                        Line::from(""),
                        Line::from(vec![
                            Span::styled(
                                format!("  {} Confs: {} (dPoW: {})  |  {} Confs: {} (dPoW: {})",
                                    base, base_confs, base_nota, rel, rel_confs, rel_nota),
                                Style::default().fg(Color::DarkGray),
                            ),
                        ]),
                        Line::from(""),
                        Line::from(vec![
                            Span::styled(
                                "  Y — PLACE ORDER  |  Esc — cancel",
                                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                            ),
                        ]),
                    ];

                    let para = Paragraph::new(lines)
                        .style(Style::default().fg(Color::White));
                    f.render_widget(para, inner);
                }
                MakerOrderModal::Sending => {
                    let block = Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Yellow))
                        .title(" Maker Order ");
                    let inner = block.inner(modal_rect);
                    f.render_widget(block, modal_rect);
                    let para = Paragraph::new("  Placing order...")
                        .style(Style::default().fg(Color::Yellow));
                    f.render_widget(para, inner);
                }
                MakerOrderModal::Result { success, message } => {
                    let color = if *success { Color::Green } else { Color::Red };
                    let title = if *success { " Order Placed " } else { " Order Error " };
                    let block = Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(color))
                        .title(title);
                    let inner = block.inner(modal_rect);
                    f.render_widget(block, modal_rect);

                    let mut lines = vec![Line::from("")];
                    for part in message.split('\n') {
                        lines.push(Line::from(vec![
                            Span::styled(part.to_string(), Style::default().fg(color)),
                        ]));
                    }
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled("Press Esc to close", Style::default().fg(Color::DarkGray)),
                    ]));

                    let para = Paragraph::new(lines)
                        .style(Style::default().fg(Color::White));
                    f.render_widget(para, inner);
                }
            }
        }

        // Order info modal
        if let Some(ref info) = self.order_info_modal {
            let area = f.size();
            let modal_w = 74.min(area.width.saturating_sub(4));
            let modal_h = 24.min(area.height.saturating_sub(4));
            let x = area.width.saturating_sub(modal_w) / 2;
            let y = area.height.saturating_sub(modal_h) / 2;
            let modal_rect = Rect::new(x, y, modal_w, modal_h);

            f.render_widget(Clear, modal_rect);
            let title = match info {
                OrderInfoModal::Loading => " Order Info (loading...) ",
                OrderInfoModal::MakerOrder { .. } => " Maker Order Info (Esc to close) ",
                OrderInfoModal::TakerOrder { .. } => " Taker Order Info (Esc to close) ",
                OrderInfoModal::Error(_) => " Order Info Error (Esc to close) ",
            };
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(title);
            let inner = block.inner(modal_rect);
            f.render_widget(block, modal_rect);

            let mut content: Vec<Line> = Vec::new();
            match info {
                OrderInfoModal::Loading => {
                    content.push(Line::from("Loading order status..."));
                }
                OrderInfoModal::MakerOrder {
                    uuid, base, rel, price, max_base_vol, min_base_vol,
                    available_amount, created_at, updated_at, cancellable,
                    cancellation_reason, started_swaps, conf_settings,
                } => {
                    content.push(Self::label_value("UUID", uuid));
                    content.push(Self::label_value("Pair", &format!("{}/{}", base, rel)));
                    content.push(Self::label_value("Price", price));
                    content.push(Self::label_value("Max Volume", max_base_vol));
                    content.push(Self::label_value("Min Volume", min_base_vol));
                    content.push(Self::label_value("Available", available_amount));
                    content.push(Self::label_value("Created", &Self::format_timestamp(*created_at)));
                    if let Some(ts) = updated_at {
                        content.push(Self::label_value("Updated", &Self::format_timestamp(*ts)));
                    }
                    content.push(Self::label_value("Cancellable", if *cancellable { "yes" } else { "no" }));
                    if let Some(reason) = cancellation_reason {
                        content.push(Self::label_value("Cancel Reason", reason));
                    }
                    if let Some((bc, bn, rc, rn)) = conf_settings {
                        content.push(Line::from(""));
                        content.push(Self::label_value("Base Confs", &format!("{} (nota: {})", bc, bn)));
                        content.push(Self::label_value("Rel Confs", &format!("{} (nota: {})", rc, rn)));
                    }
                    if !started_swaps.is_empty() {
                        content.push(Line::from(""));
                        content.push(Line::from(Span::styled(
                            "Started Swaps:",
                            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                        )));
                        for s in started_swaps {
                            content.push(Line::from(format!("  {}", s)));
                        }
                    }
                }
                OrderInfoModal::TakerOrder {
                    uuid, base, rel, base_amount, rel_amount, action,
                    created_at, cancellable, order_type, cancellation_reason,
                } => {
                    content.push(Self::label_value("UUID", uuid));
                    content.push(Self::label_value("Pair", &format!("{}/{}", base, rel)));
                    content.push(Self::label_value("Action", action));
                    content.push(Self::label_value("Base Amount", base_amount));
                    content.push(Self::label_value("Rel Amount", rel_amount));
                    content.push(Self::label_value("Created", &Self::format_timestamp(*created_at)));
                    content.push(Self::label_value("Cancellable", if *cancellable { "yes" } else { "no" }));
                    if let Some(ot) = order_type {
                        content.push(Self::label_value("Order Type", ot));
                    }
                    if let Some(reason) = cancellation_reason {
                        content.push(Self::label_value("Cancellation", reason));
                    }
                }
                OrderInfoModal::Error(msg) => {
                    content.push(Line::from(Span::styled(
                        msg.clone(),
                        Style::default().fg(Color::Red),
                    )));
                }
            }
            content.push(Line::from(""));
            content.push(Line::from(Span::styled(
                "Press Esc to close",
                Style::default().fg(Color::DarkGray),
            )));

            let para = Paragraph::new(content)
                .style(Style::default().fg(Color::White));
            f.render_widget(para, inner);
        }
    }

    fn label_value<'a>(label: &str, value: &str) -> Line<'a> {
        Line::from(vec![
            Span::styled(
                format!("{:>14}: ", label),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::raw(value.to_string()),
        ])
    }

    fn format_timestamp(ts: u64) -> String {
        use chrono::{DateTime, Utc};
        // KDF returns timestamps in milliseconds
        let secs = (ts / 1000) as i64;
        DateTime::<Utc>::from_timestamp(secs, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
            .unwrap_or_else(|| ts.to_string())
    }
}
