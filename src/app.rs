use chrono::{DateTime, Local};
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, HighlightSpacing, List, ListItem, ListState, Paragraph},
    Frame,
};
use crate::coins::Coin;
use crate::logger::SharedLogger;
use std::sync::{Arc, Mutex};

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
        self.last_key_pressed = key;
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
    
    pub fn render(&self, f: &mut Frame) {
        // Split into: main area, log area, status bar
        let chunks = Layout::default()
            .constraints([
                Constraint::Min(0),
                Constraint::Length(8),
                Constraint::Length(3),
            ])
            .split(f.size());
        
        // Main area: left = coins + balances (ticker left, balance right), right = Details + TX History
        let main_chunks = Layout::default()
            .direction(ratatui::layout::Direction::Horizontal)
            .constraints([Constraint::Length(28), Constraint::Min(0)])
            .split(chunks[0]);
        
        // Split right panel into Details (top) and Transaction History (bottom)
        let right_chunks = Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([Constraint::Length(10), Constraint::Min(0)])
            .split(main_chunks[1]);
        let coins_block = Block::default()
            .borders(Borders::ALL)
            .title(" Coins (↑/↓ Enter) ");
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
            .title(" Details (R - refresh, I - info) ");
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
        
        // Log area
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
            
            f.render_stateful_widget(log_list, chunks[1], &mut list_state);
            
            // Update state
            *self.log_list_state.lock().unwrap() = list_state;
        }
        
        // Status bar: version | last key | time
        let status_chunks = Layout::default()
            .direction(ratatui::layout::Direction::Horizontal)
            .constraints([
                Constraint::Percentage(40),
                Constraint::Percentage(20),
                Constraint::Percentage(40),
            ])
            .split(chunks[2]);
        
        let version_text = format!("KDF: {}", self.kdf_version);
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
                    let input = Paragraph::new(password.as_str())
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
    }
}
