mod app;
mod coins;
mod config;
mod file_manager;
mod kdf_client;
mod logger;
mod qr_compact;

use anyhow::Result;
use app::App;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::Rect,
    Terminal,
};
use std::fs::OpenOptions;
use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, RwLock};
use tokio::time::{interval, Duration};
use logger::create_logger;

const POLL_INTERVAL_SECONDS: u64 = 30;

async fn check_and_kill_existing_kdf(logger: &logger::SharedLogger) -> Result<()> {
    // Check if KDF process is already running using pgrep
    let pgrep_output = Command::new("pgrep")
        .arg("-f")
        .arg("kdf")
        .output();
    
    match pgrep_output {
        Ok(output) if output.status.success() => {
            let pids = String::from_utf8_lossy(&output.stdout);
            let pids: Vec<&str> = pids.trim().split('\n').filter(|s| !s.is_empty()).collect();
            
            if !pids.is_empty() {
                if let Ok(mut log) = logger.write() {
                    log.info(format!("Found {} existing KDF process(es) with PID(s): {}", pids.len(), pids.join(", ")));
                    log.info("Terminating existing KDF processes...".to_string());
                }
                
                // Kill existing processes
                let kill_result = Command::new("pkill")
                    .arg("-f")
                    .arg("kdf")
                    .output();
                
                match kill_result {
                    Ok(output) if output.status.success() => {
                        if let Ok(mut log) = logger.write() {
                            log.info("Successfully terminated existing KDF processes".to_string());
                        }
                        // Wait a bit for processes to terminate
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                    Ok(_) => {
                        if let Ok(mut log) = logger.write() {
                            log.warn("Failed to terminate some KDF processes".to_string());
                        }
                    }
                    Err(e) => {
                        if let Ok(mut log) = logger.write() {
                            log.warn(format!("Error while terminating KDF processes: {}", e));
                        }
                    }
                }
            } else {
                if let Ok(mut log) = logger.write() {
                    log.info("No existing KDF processes found".to_string());
                }
            }
        }
        Ok(_) => {
            // pgrep returned non-zero, no processes found
            if let Ok(mut log) = logger.write() {
                log.info("No existing KDF processes found".to_string());
            }
        }
        Err(e) => {
            // pgrep command failed, assume no processes
            if let Ok(mut log) = logger.write() {
                log.warn(format!("Could not check for existing KDF processes (pgrep failed: {}), assuming none running", e));
            }
        }
    }
    
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let workspace_path = std::env::current_dir()?;
    
    // Create logger
    let logger = create_logger(100);
    
    // Log startup
    {
        let mut log = logger.write().unwrap();
        log.info("Starting MM2-RTUI application".to_string());
    }
    
    // Check and download required files
    {
        let mut log = logger.write().unwrap();
        log.info("Checking required files...".to_string());
    }
    
    file_manager::ensure_required_files(&workspace_path, &logger).await?;
    
    // Setup MM2.json
    {
        let mut log = logger.write().unwrap();
        log.info("Setting up MM2.json configuration...".to_string());
    }
    
    let mm2_path = workspace_path.join("MM2.json");
    let rpc_password = config::setup_mm2_config(&mm2_path, &workspace_path, &logger).await?;
    
    // Set environment variables
    let coins_path = workspace_path.join("coins.json");
    std::env::set_var("MM_COINS_PATH", coins_path.to_str().unwrap());
    std::env::set_var("MM_CONF_PATH", mm2_path.to_str().unwrap());
    
    {
        let mut log = logger.write().unwrap();
        log.info(format!("Set MM_COINS_PATH: {}", coins_path.display()));
        log.info(format!("Set MM_CONF_PATH: {}", mm2_path.display()));
        log.info(format!("RPC password: {}", rpc_password));
    }
    
    // Check and kill existing KDF processes
    {
        let mut log = logger.write().unwrap();
        log.info("Checking for existing KDF processes...".to_string());
    }
    check_and_kill_existing_kdf(&logger).await?;
    
    // Start KDF process
    let kdf_path = workspace_path.join("kdf");
    let kdf_log_path = workspace_path.join("kdf.log");
    {
        let mut log = logger.write().unwrap();
        log.info(format!("Starting KDF binary: {}", kdf_path.display()));
        log.info(format!("KDF stdout/stderr → {}", kdf_log_path.display()));
    }
    let kdf_log = OpenOptions::new()
        .create(true)
        .append(true)
        .write(true)
        .open(&kdf_log_path)?;
    let kdf_log_stderr = OpenOptions::new()
        .create(true)
        .append(true)
        .write(true)
        .open(&kdf_log_path)?;
    let kdf_process = Command::new(&kdf_path)
        .stdout(Stdio::from(kdf_log))
        .stderr(Stdio::from(kdf_log_stderr))
        .spawn()?;
    
    let kdf_pid = kdf_process.id();
    {
        let mut log = logger.write().unwrap();
        log.info(format!("KDF process started successfully with PID: {}", kdf_pid));
    }
    
    // Wait a bit for KDF to start and read MM2.json
    {
        let mut log = logger.write().unwrap();
        log.info("Waiting for KDF to initialize (reading MM2.json)...".to_string());
    }
    tokio::time::sleep(Duration::from_secs(3)).await;
    
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture, crossterm::event::EnableBracketedPaste)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    
    // Create app state
    let app_state = Arc::new(RwLock::new(App::new(logger.clone())));
    
    // Log RPC password again now that UI is visible
    {
        let mut log = logger.write().unwrap();
        log.info(format!("RPC password: {}", rpc_password));
        log.info("Attempting to connect to KDF...".to_string());
    }
    terminal.draw(|f| {
        if let Ok(app) = app_state.read() {
            app.render(f);
        }
    })?;
    
    // Initial poll with retries
    {
        let mut retries = 0;
        const MAX_RETRIES: u32 = 5;
        let mut connected = false;
        
        while retries < MAX_RETRIES && !connected {
            match kdf_client::get_version(&rpc_password).await {
                Ok(version_info) => {
                    let version = version_info.result.clone();
                    if let Ok(mut app) = app_state.write() {
                        app.update_version(version.clone());
                        app.update_datetime(version_info.datetime);
                    }
                    if let Ok(mut log) = logger.write() {
                        log.info("Successfully connected to KDF".to_string());
                        log.info(format!("KDF version: {}", version));
                    }
                    // Request wallet list and open selection modal
                    match kdf_client::get_wallet_names(&rpc_password).await {
                        Ok(res) => {
                            if let Ok(mut app) = app_state.write() {
                                app.open_wallet_modal(res.result.wallet_names);
                            }
                            if let Ok(mut log) = logger.write() {
                                log.info("Loaded wallet list, please select a wallet".to_string());
                            }
                        }
                        Err(e) => {
                            if let Ok(mut log) = logger.write() {
                                log.warn(format!("Failed to get wallet names: {}", e));
                            }
                        }
                    }
                    terminal.draw(|f| {
                        if let Ok(app) = app_state.read() {
                            app.render(f);
                        }
                    })?;
                    connected = true;
                }
                Err(e) => {
                    retries += 1;
                    if let Ok(mut log) = logger.write() {
                        if retries < MAX_RETRIES {
                            log.warn(format!("Initial KDF version check failed (attempt {}/{}): {}", retries, MAX_RETRIES, e));
                            log.info("Retrying in 1 second...".to_string());
                        } else {
                            log.error(format!("Failed to connect to KDF after {} attempts: {}", MAX_RETRIES, e));
                            log.error("Please check if KDF is running and MM2.json is correct".to_string());
                        }
                    }
                    terminal.draw(|f| {
                        if let Ok(app) = app_state.read() {
                            app.render(f);
                        }
                    })?;
                    if retries < MAX_RETRIES {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        }
    }
    
    // Start polling task
    let app_state_clone = app_state.clone();
    let logger_clone = logger.clone();
    let rpc_password_clone = rpc_password.clone();
    let poll_handle = tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(POLL_INTERVAL_SECONDS));
        loop {
            interval.tick().await;
            match kdf_client::get_version(&rpc_password_clone).await {
                Ok(version_info) => {
                    if let Ok(mut app) = app_state_clone.write() {
                        app.update_version(version_info.result.clone());
                        app.update_datetime(version_info.datetime);
                    }
                }
                Err(e) => {
                    if let Ok(mut log) = logger_clone.write() {
                        log.warn(format!("Failed to get KDF version: {}", e));
                    }
                }
            }
        }
    });
    
    // Main event loop
    let result = run_app(
        &mut terminal,
        app_state.clone(),
        rpc_password.clone(),
        logger.clone(),
        mm2_path.clone(),
        workspace_path.clone(),
    )
    .await;
    
    // Cleanup
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        crossterm::event::DisableBracketedPaste
    )?;
    terminal.show_cursor()?;
    
    // Stop polling
    poll_handle.abort();
    
    result
}

fn key_code_to_display(code: KeyCode) -> String {
    use crossterm::event::KeyCode as KC;
    match code {
        KC::Char(c) => c.to_string(),
        KC::Backspace => "Backspace".to_string(),
        KC::Enter => "Enter".to_string(),
        KC::Left => "Left".to_string(),
        KC::Right => "Right".to_string(),
        KC::Up => "Up".to_string(),
        KC::Down => "Down".to_string(),
        KC::Home => "Home".to_string(),
        KC::End => "End".to_string(),
        KC::PageUp => "PgUp".to_string(),
        KC::PageDown => "PgDn".to_string(),
        KC::Tab => "Tab".to_string(),
        KC::BackTab => "BackTab".to_string(),
        KC::Delete => "Delete".to_string(),
        KC::Insert => "Insert".to_string(),
        KC::F(n) => format!("F{}", n),
        KC::Null => "Null".to_string(),
        KC::Esc => "Esc".to_string(),
        KC::CapsLock => "CapsLock".to_string(),
        KC::ScrollLock => "ScrollLock".to_string(),
        KC::NumLock => "NumLock".to_string(),
        KC::PrintScreen => "PrintScreen".to_string(),
        KC::Pause => "Pause".to_string(),
        KC::Menu => "Menu".to_string(),
        KC::KeypadBegin => "KeypadBegin".to_string(),
        KC::Modifier(_) => "Modifier".to_string(),
        KC::Media(_) => "Media".to_string(),
    }
}

async fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    app_state: Arc<RwLock<App>>,
    rpc_password: String,
    logger: logger::SharedLogger,
    mm2_path: PathBuf,
    workspace_path: PathBuf,
) -> Result<()> {
    let poll_timeout = std::time::Duration::from_millis(16);
    let zero = std::time::Duration::ZERO;

    loop {
        // Block for first event, then drain all pending events so Key isn't stuck behind Resize
        let had_events = crossterm::event::poll(poll_timeout)?;
        let mut last_resize: Option<(u16, u16)> = None;

        if had_events {
            loop {
                match event::read()? {
                    Event::Resize(cols, rows) => {
                        last_resize = Some((cols, rows));
                    }
                    Event::Paste(text) => {
                        // Handle bracketed paste (Ctrl+Shift+V in most terminals)
                        if let Ok(mut app) = app_state.write() {
                            if app.withdraw_modal().is_some() {
                                for c in text.chars() {
                                    if !c.is_control() {
                                        app.withdraw_modal_push_char(c);
                                    }
                                }
                            } else if matches!(app.wallet_modal(), Some(app::WalletModalState::EnteringPassword { .. })) {
                                for c in text.chars() {
                                    if !c.is_control() {
                                        app.wallet_modal_password_push(c);
                                    }
                                }
                            }
                        }
                    }
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        let key_name = key_code_to_display(key.code);
                        if let Ok(mut app) = app_state.write() {
                            app.set_last_key(key_name.clone());
                            // Handle wallet modal keys when modal is open
                            if app.wallet_modal().is_some() {
                                match key.code {
                                    KeyCode::Up => app.wallet_modal_select_up(),
                                    KeyCode::Down => app.wallet_modal_select_down(),
                                    KeyCode::Char('h') | KeyCode::Char('H') => {
                                        if matches!(app.wallet_modal(), Some(app::WalletModalState::Selecting { .. }) ) {
                                            app.wallet_modal_toggle_hd();
                                        } else {
                                            let c = if key.code == KeyCode::Char('H') { 'H' } else { 'h' };
                                            app.wallet_modal_password_push(c);
                                        }
                                    }
                                    KeyCode::Enter => {
                                        let submitted = if matches!(app.wallet_modal(), Some(app::WalletModalState::Selecting { .. }) ) {
                                            app.wallet_modal_confirm_selection();
                                            None
                                        } else if matches!(app.wallet_modal(), Some(app::WalletModalState::EnteringPassword { .. }) ) {
                                            app.wallet_modal_submit_password()
                                        } else {
                                            None
                                        };
                                        if let Some((name, pass, enable_hd, names)) = submitted {
                                            drop(app);
                                            if let Ok(mut log) = logger.write() {
                                                log.info(format!("Selected wallet: {} | enable_hd: {}", name, enable_hd));
                                            }
                                            if let Err(e) = terminal.draw(|f| {
                                                if let Ok(app) = app_state.read() {
                                                    app.render(f);
                                                }
                                            }) {
                                                eprintln!("Error drawing terminal: {}", e);
                                            }
                                            // Kill existing KDF, update MM2.json with wallet, restart KDF
                                            if let Ok(mut log) = logger.write() {
                                                log.info("Stopping KDF to apply wallet selection...".to_string());
                                            }
                                            if let Err(e) = terminal.draw(|f| {
                                                if let Ok(app) = app_state.read() {
                                                    app.render(f);
                                                }
                                            }) {
                                                eprintln!("Error drawing terminal: {}", e);
                                            }
                                            if let Err(e) = check_and_kill_existing_kdf(&logger).await {
                                                if let Ok(mut log) = logger.write() {
                                                    log.warn(format!("Failed to kill existing KDF: {}", e));
                                                }
                                            }
                                            if let Err(e) =
                                                config::update_mm2_wallet(&mm2_path, &name, &pass, enable_hd, &logger).await
                                            {
                                                if let Ok(mut log) = logger.write() {
                                                    log.error(format!("Failed to update MM2.json: {}", e));
                                                }
                                                if let Err(e) = terminal.draw(|f| {
                                                    if let Ok(app) = app_state.read() {
                                                        app.render(f);
                                                    }
                                                }) {
                                                    eprintln!("Error drawing terminal: {}", e);
                                                }
                                            } else {
                                                let kdf_path = workspace_path.join("kdf");
                                                let kdf_log_path = workspace_path.join("kdf.log");
                                                if let Ok(mut log) = logger.write() {
                                                    log.info(format!("Restarting KDF: {}", kdf_path.display()));
                                                    log.info(format!("KDF stdout/stderr → {}", kdf_log_path.display()));
                                                }
                                                let kdf_log_stdout = OpenOptions::new()
                                                    .create(true)
                                                    .append(true)
                                                    .write(true)
                                                    .open(&kdf_log_path);
                                                let kdf_log_stderr = OpenOptions::new()
                                                    .create(true)
                                                    .append(true)
                                                    .write(true)
                                                    .open(&kdf_log_path);
                                                match (kdf_log_stdout, kdf_log_stderr) {
                                                    (Ok(stdout), Ok(stderr)) => match Command::new(&kdf_path)
                                                        .stdout(Stdio::from(stdout))
                                                        .stderr(Stdio::from(stderr))
                                                        .spawn()
                                                    {
                                                    Ok(mut process) => {
                                                        let pid = process.id();
                                                        if let Ok(mut log) = logger.write() {
                                                            log.info(format!("KDF restarted with PID: {}", pid));
                                                            log.info("Waiting for KDF to initialize...".to_string());
                                                        }
                                                        if let Err(e) = terminal.draw(|f| {
                                                            if let Ok(app) = app_state.read() {
                                                                app.render(f);
                                                            }
                                                        }) {
                                                            eprintln!("Error drawing terminal: {}", e);
                                                        }
                                                        tokio::time::sleep(Duration::from_secs(3)).await;
                                                        if let Ok(Some(status)) = process.try_wait() {
                                                            if !status.success() {
                                                                let code = status.code().unwrap_or(-1);
                                                                if let Ok(mut log) = logger.write() {
                                                                    log.error(format!("KDF exited with error code: {} (e.g. wrong password?)", code));
                                                                    log.info("Returning to wallet selection".to_string());
                                                                }
                                                                if let Ok(mut app) = app_state.write() {
                                                                    app.open_wallet_modal(names);
                                                                }
                                                            } else if let Ok(mut log) = logger.write() {
                                                                log.info("KDF restart complete".to_string());
                                                            }
                                                        } else if let Ok(mut log) = logger.write() {
                                                            log.info("KDF restart complete".to_string());
                                                        }
                                                        // Start default UTXO coin activation (KMD, KMDCL)
                                                        let coins_config_path = workspace_path.join("coins_config.json");
                                                        match coins::load_utxo_coins_from_config(
                                                            &coins_config_path,
                                                            coins::DEFAULT_TICKERS,
                                                        ) {
                                                            Ok(list) => {
                                                                for (coin, params) in list {
                                                                    let ticker = coin.ticker.clone();
                                                                    if let Ok(mut app) = app_state.write() {
                                                                        app.add_coin(coin);
                                                                    }
                                                                    if let Ok(mut log) = logger.write() {
                                                                        log.info(format!("Activating UTXO coin: {}", ticker));
                                                                    }
                                                                    match kdf_client::task_enable_utxo_init(
                                                                        &rpc_password,
                                                                        &ticker,
                                                                        params,
                                                                    )
                                                                    .await
                                                                    {
                                                                        Ok(res) => {
                                                                            let task_id = res.result.task_id;
                                                                            if let Ok(mut app) = app_state.write() {
                                                                                app.add_pending_task(task_id, ticker.clone());
                                                                            }
                                                                            if let Ok(mut log) = logger.write() {
                                                                                log.info(format!("task_id {} for {}", task_id, ticker));
                                                                            }
                                                                        }
                                                                        Err(e) => {
                                                                            if let Ok(mut log) = logger.write() {
                                                                                log.error(format!("task::enable_utxo::init {}: {}", ticker, e));
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                            Err(e) => {
                                                                if let Ok(mut log) = logger.write() {
                                                                    log.warn(format!("Loading coins_config for activation: {}", e));
                                                                }
                                                            }
                                                        }
                                                    }
                                                    Err(e) => {
                                                        if let Ok(mut log) = logger.write() {
                                                            log.error(format!("Failed to start KDF: {}", e));
                                                        }
                                                    }
                                                },
                                                    _ => {
                                                        if let Ok(mut log) = logger.write() {
                                                            log.error("Failed to open kdf.log for KDF restart".to_string());
                                                        }
                                                    }
                                                }
                                            }
                                            if let Err(e) = terminal.draw(|f| {
                                                if let Ok(app) = app_state.read() {
                                                    app.render(f);
                                                }
                                            }) {
                                                eprintln!("Error drawing terminal: {}", e);
                                            }
                                        }
                                    }
                                    KeyCode::Esc => app.wallet_modal_close(),
                                    KeyCode::Char(c) => app.wallet_modal_password_push(c),
                                    KeyCode::Backspace => app.wallet_modal_password_backspace(),
                                    _ => {}
                                }
                                // Skip default key handling for modal
                            } else if app.coin_select_modal().is_some() {
                                match key.code {
                                    KeyCode::Esc => {
                                        app.close_coin_select_modal();
                                    }
                                    KeyCode::Up => {
                                        if let Some(m) = app.coin_select_modal_mut() {
                                            m.move_up();
                                        }
                                    }
                                    KeyCode::Down => {
                                        if let Some(m) = app.coin_select_modal_mut() {
                                            m.move_down();
                                        }
                                    }
                                    KeyCode::Char(' ') => {
                                        if let Some(m) = app.coin_select_modal_mut() {
                                            m.toggle_selected();
                                        }
                                    }
                                    KeyCode::Enter => {
                                        let selected = app.coin_select_modal_confirm();
                                        if !selected.is_empty() {
                                            drop(app);
                                            if let Ok(mut log) = logger.write() {
                                                log.info(format!("Activating {} coin(s): {:?}", selected.len(), selected));
                                            }
                                            let coins_config_path = workspace_path.join("coins_config.json");
                                            match coins::load_utxo_coins_from_config_owned(&coins_config_path, &selected) {
                                                Ok(list) => {
                                                    for (coin, params) in list {
                                                        let ticker = coin.ticker.clone();
                                                        if let Ok(mut a) = app_state.write() {
                                                            a.add_coin(coin);
                                                        }
                                                        if let Ok(mut log) = logger.write() {
                                                            log.info(format!("Activating UTXO coin: {}", ticker));
                                                        }
                                                        match kdf_client::task_enable_utxo_init(
                                                            &rpc_password,
                                                            &ticker,
                                                            params,
                                                        )
                                                        .await
                                                        {
                                                            Ok(res) => {
                                                                let task_id = res.result.task_id;
                                                                if let Ok(mut a) = app_state.write() {
                                                                    a.add_pending_task(task_id, ticker.clone());
                                                                }
                                                                if let Ok(mut log) = logger.write() {
                                                                    log.info(format!("task_id {} for {}", task_id, ticker));
                                                                }
                                                            }
                                                            Err(e) => {
                                                                if let Ok(mut log) = logger.write() {
                                                                    log.error(format!("task::enable_utxo::init {}: {}", ticker, e));
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    if let Ok(mut log) = logger.write() {
                                                        log.error(format!("Loading coins_config for activation: {}", e));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    KeyCode::Backspace => {
                                        if let Some(m) = app.coin_select_modal_mut() {
                                            m.filter_backspace();
                                        }
                                    }
                                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                                        if let Some(m) = app.coin_select_modal_mut() {
                                            m.push_filter_char(c);
                                        }
                                    }
                                    _ => {}
                                }
                            } else if app.withdraw_modal().is_some() {
                                match key.code {
                                    KeyCode::Esc => {
                                        app.close_withdraw_modal();
                                    }
                                    KeyCode::Enter => {
                                        // Handle Enter based on current withdraw modal state
                                        match app.withdraw_modal() {
                                            Some(app::WithdrawModalState::EnteringAddress { .. }) => {
                                                app.withdraw_modal_confirm_address();
                                            }
                                            Some(app::WithdrawModalState::EnteringAmount { .. }) => {
                                                if let Some((ticker, address, amount)) = app.withdraw_modal_confirm_amount() {
                                                    drop(app);
                                                    if let Ok(mut log) = logger.write() {
                                                        log.info(format!("Withdraw {} {} to {}", amount, ticker, address));
                                                    }
                                                    match kdf_client::withdraw(&rpc_password, &ticker, &address, &amount).await {
                                                        Ok(result) => {
                                                            if let Ok(mut log) = logger.write() {
                                                                log.info(format!("Withdraw prepared: tx_hash={}", result.tx_hash));
                                                            }
                                                            if let Ok(mut a) = app_state.write() {
                                                                a.withdraw_modal_set_confirmation(ticker, result);
                                                            }
                                                        }
                                                        Err(e) => {
                                                            if let Ok(mut log) = logger.write() {
                                                                log.error(format!("Withdraw failed: {}", e));
                                                            }
                                                            if let Ok(mut a) = app_state.write() {
                                                                a.withdraw_modal_set_error(format!("{}", e));
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                    KeyCode::Char('y') | KeyCode::Char('Y') => {
                                        // Confirm send in Confirming state
                                        if let Some((ticker, coin, tx_hex)) = app.withdraw_modal_confirm_send() {
                                            drop(app);
                                            if let Ok(mut log) = logger.write() {
                                                log.info(format!("Sending raw transaction for {}...", ticker));
                                            }
                                            match kdf_client::send_raw_transaction(&rpc_password, &coin, &tx_hex).await {
                                                Ok(res) => {
                                                    if let Ok(mut log) = logger.write() {
                                                        log.info(format!("Transaction sent! tx_hash: {}", res.tx_hash));
                                                    }
                                                    if let Ok(mut a) = app_state.write() {
                                                        a.withdraw_modal_set_result(
                                                            true,
                                                            format!("Transaction sent successfully!\n\n{}", res.tx_hash),
                                                        );
                                                    }
                                                }
                                                Err(e) => {
                                                    if let Ok(mut log) = logger.write() {
                                                        log.error(format!("send_raw_transaction failed: {}", e));
                                                    }
                                                    if let Ok(mut a) = app_state.write() {
                                                        a.withdraw_modal_set_result(false, format!("Send failed: {}", e));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                                        app.withdraw_modal_push_char(c);
                                    }
                                    KeyCode::Backspace => {
                                        app.withdraw_modal_backspace();
                                    }
                                    _ => {}
                                }
                            } else {
                                match key.code {
                                    KeyCode::Char('q') | KeyCode::Char('Q') => {
                            // Release write lock before shutdown so terminal.draw can read app_state
                            drop(app);
                            // Graceful shutdown - log immediately and redraw
                            {
                                let mut log = logger.write().unwrap();
                                log.info("User requested shutdown (Q/Esc pressed)".to_string());
                                log.info("Initiating graceful KDF shutdown...".to_string());
                            }
                            // Force immediate redraw to show the log message
                            if let Err(e) = terminal.draw(|f| {
                                if let Ok(app) = app_state.read() {
                                    app.render(f);
                                }
                            }) {
                                eprintln!("Error drawing terminal: {}", e);
                            }
                            
                            // Call stop method
                            {
                                let mut log = logger.write().unwrap();
                                log.info("Sending stop command to KDF...".to_string());
                            }
                            if let Err(e) = terminal.draw(|f| {
                                if let Ok(app) = app_state.read() {
                                    app.render(f);
                                }
                            }) {
                                eprintln!("Error drawing terminal: {}", e);
                            }
                            
                            match kdf_client::stop(&rpc_password).await {
                                Ok(stop_response) => {
                                    {
                                        let mut log = logger.write().unwrap();
                                        log.info(format!("KDF stop command accepted. Response: {}", stop_response.result));
                                    }
                                }
                                Err(e) => {
                                    {
                                        let mut log = logger.write().unwrap();
                                        log.warn(format!("Failed to send stop command to KDF: {}", e));
                                        log.warn("KDF may already be stopped or unreachable".to_string());
                                    }
                                }
                            }
                            if let Err(e) = terminal.draw(|f| {
                                if let Ok(app) = app_state.read() {
                                    app.render(f);
                                }
                            }) {
                                eprintln!("Error drawing terminal: {}", e);
                            }
                            
                            // Wait a bit and verify KDF is stopped
                            {
                                let mut log = logger.write().unwrap();
                                log.info("Waiting for KDF to stop...".to_string());
                            }
                            if let Err(e) = terminal.draw(|f| {
                                if let Ok(app) = app_state.read() {
                                    app.render(f);
                                }
                            }) {
                                eprintln!("Error drawing terminal: {}", e);
                            }
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            
                            // Verify KDF is stopped by trying to get version
                            let mut attempts = 0;
                            const MAX_ATTEMPTS: u32 = 5;
                            
                            {
                                let mut log = logger.write().unwrap();
                                log.info(format!("Verifying KDF shutdown (max {} attempts)...", MAX_ATTEMPTS));
                            }
                            if let Err(e) = terminal.draw(|f| {
                                if let Ok(app) = app_state.read() {
                                    app.render(f);
                                }
                            }) {
                                eprintln!("Error drawing terminal: {}", e);
                            }
                            
                            while attempts < MAX_ATTEMPTS {
                                match kdf_client::get_version(&rpc_password).await {
                                    Ok(_) => {
                                        // KDF still responding, wait a bit more
                                        {
                                            let mut log = logger.write().unwrap();
                                            log.info(format!("KDF still responding, waiting... (attempt {}/{})", attempts + 1, MAX_ATTEMPTS));
                                        }
                                        if let Err(e) = terminal.draw(|f| {
                                            if let Ok(app) = app_state.read() {
                                                app.render(f);
                                            }
                                        }) {
                                            eprintln!("Error drawing terminal: {}", e);
                                        }
                                        tokio::time::sleep(Duration::from_secs(1)).await;
                                        attempts += 1;
                                    }
                                    Err(e) => {
                                        // KDF is stopped
                                        {
                                            let mut log = logger.write().unwrap();
                                            log.info(format!("KDF stopped successfully (no response on attempt {})", attempts + 1));
                                            log.info(format!("Shutdown verification: {}", e));
                                        }
                                        if let Err(e) = terminal.draw(|f| {
                                            if let Ok(app) = app_state.read() {
                                                app.render(f);
                                            }
                                        }) {
                                            eprintln!("Error drawing terminal: {}", e);
                                        }
                                        break;
                                    }
                                }
                            }
                            
                            if attempts >= MAX_ATTEMPTS {
                                {
                                    let mut log = logger.write().unwrap();
                                    log.warn(format!("KDF did not stop within {} attempts, but continuing shutdown", MAX_ATTEMPTS));
                                }
                            } else {
                                {
                                    let mut log = logger.write().unwrap();
                                    log.info("KDF shutdown verified successfully".to_string());
                                }
                            }
                            if let Err(e) = terminal.draw(|f| {
                                if let Ok(app) = app_state.read() {
                                    app.render(f);
                                }
                            }) {
                                eprintln!("Error drawing terminal: {}", e);
                            }
                            
                            {
                                let mut log = logger.write().unwrap();
                                log.info("Cleaning up application resources...".to_string());
                            }
                            if let Err(e) = terminal.draw(|f| {
                                if let Ok(app) = app_state.read() {
                                    app.render(f);
                                }
                            }) {
                                eprintln!("Error drawing terminal: {}", e);
                            }
                            
                            {
                                let mut log = logger.write().unwrap();
                                log.info("Application shutdown complete".to_string());
                            }
                            if let Err(e) = terminal.draw(|f| {
                                if let Ok(app) = app_state.read() {
                                    app.render(f);
                                }
                            }) {
                                eprintln!("Error drawing terminal: {}", e);
                            }
                            
                            // Give user a moment to see the final messages
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            
                            return Ok(());
                        }
                        KeyCode::Char('r') | KeyCode::Char('R') => {
                            let ticker = app.selected_coin_ticker();
                            if let Some(ticker) = ticker {
                                drop(app);
                                match kdf_client::my_balance(&rpc_password, &ticker).await {
                                    Ok(res) => {
                                        let (spend, unspend) =
                                            coins::my_balance_to_satoshis(&res.balance, &res.unspendable_balance);
                                        if let Ok(mut app) = app_state.write() {
                                            app.update_coin_from_my_balance(
                                                &ticker,
                                                spend,
                                                unspend,
                                                res.address,
                                            );
                                        }
                                        if let Ok(mut log) = logger.write() {
                                            log.info(format!("Refreshed balance for {}", ticker));
                                        }
                                    }
                                    Err(e) => {
                                        if let Ok(mut log) = logger.write() {
                                            log.warn(format!("my_balance {}: {}", ticker, e));
                                        }
                                    }
                                }
                                // Also fetch transaction history
                                if let Ok(mut app) = app_state.write() {
                                    app.set_tx_history_loading(true);
                                }
                                match kdf_client::my_tx_history(&rpc_password, &ticker, 10, Some(1)).await {
                                    Ok(res) => {
                                        if let Ok(mut app) = app_state.write() {
                                            let total_pages = res.result.total_pages;
                                            let page = res.result.paging_options.page_number;
                                            let current_block = res.result.current_block;
                                            app.update_tx_history(res.result.transactions, page, total_pages, current_block);
                                        }
                                        if let Ok(mut log) = logger.write() {
                                            log.info(format!("Refreshed transaction history for {}", ticker));
                                        }
                                    }
                                    Err(e) => {
                                        if let Ok(mut app) = app_state.write() {
                                            app.set_tx_history_error(format!("Failed to fetch transactions: {}", e));
                                        }
                                        if let Ok(mut log) = logger.write() {
                                            log.warn(format!("my_tx_history {}: {}", ticker, e));
                                        }
                                    }
                                }
                                break;
                            }
                        }
                        KeyCode::Char('i') | KeyCode::Char('I') => {
                            // Open information modal for selected coin
                            if app.coins_selected_index().is_some() {
                                app.open_info_modal();
                            }
                        }
                        KeyCode::Char('w') | KeyCode::Char('W') => {
                            // Open withdraw modal for selected coin
                            if let Some(ticker) = app.selected_coin_ticker() {
                                app.open_withdraw_modal(ticker);
                            }
                        }
                        KeyCode::Char('+') | KeyCode::Char('=') => {
                            // Open coin activation modal
                            let coins_json_path = workspace_path.join("coins.json");
                            match coins::load_utxo_coin_list(&coins_json_path) {
                                Ok(coin_list) => {
                                    app.open_coin_select_modal(coin_list);
                                }
                                Err(e) => {
                                    drop(app);
                                    if let Ok(mut log) = logger.write() {
                                        log.error(format!("Failed to load coins.json: {}", e));
                                    }
                                }
                            }
                        }
                        KeyCode::Esc => {
                            // Close information modal if open, otherwise quit
                            if app.is_info_modal_open() {
                                app.close_info_modal();
                            } else {
                                // Original quit logic
                                drop(app);
                                {
                                    let mut log = logger.write().unwrap();
                                    log.info("User requested shutdown (Esc pressed)".to_string());
                                    log.info("Initiating graceful KDF shutdown...".to_string());
                                }
                                if let Err(e) = terminal.draw(|f| {
                                    if let Ok(app) = app_state.read() {
                                        app.render(f);
                                    }
                                }) {
                                    eprintln!("Error drawing terminal: {}", e);
                                }
                                match kdf_client::stop(&rpc_password).await {
                                    Ok(_) => {
                                        let mut log = logger.write().unwrap();
                                        log.info("KDF stop command sent successfully".to_string());
                                    }
                                    Err(e) => {
                                        let mut log = logger.write().unwrap();
                                        log.warn(format!("Failed to send stop command: {}", e));
                                    }
                                }
                                if let Err(e) = terminal.draw(|f| {
                                    if let Ok(app) = app_state.read() {
                                        app.render(f);
                                    }
                                }) {
                                    eprintln!("Error drawing terminal: {}", e);
                                }
                                tokio::time::sleep(Duration::from_millis(500)).await;
                                return Ok(());
                            }
                        }
                        KeyCode::Up => {
                            app.coins_select_up();
                            app.clear_tx_history();
                        }
                        KeyCode::Down => {
                            app.coins_select_down();
                            app.clear_tx_history();
                        }
                        KeyCode::PageUp => {
                            let entry_count = logger.read().map(|l| l.get_entries().len()).unwrap_or(0);
                            app.scroll_log_up(entry_count);
                        }
                        KeyCode::PageDown => {
                            let entry_count = logger.read().map(|l| l.get_entries().len()).unwrap_or(0);
                            app.scroll_log_down(entry_count);
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') => {
                            // Next page of transaction history
                            let ticker = app.selected_coin_ticker();
                            if ticker.is_some() {
                                app.tx_history_next_page();
                                let page = app.tx_history_page();
                                drop(app);
                                if let Some(ticker) = ticker {
                                    // Fetch transaction history for new page
                                    if let Ok(mut app) = app_state.write() {
                                        app.set_tx_history_loading(true);
                                    }
                                    match kdf_client::my_tx_history(&rpc_password, &ticker, 10, Some(page)).await {
                                        Ok(res) => {
                                            if let Ok(mut app) = app_state.write() {
                                                let total_pages = res.result.total_pages;
                                                let page = res.result.paging_options.page_number;
                                                let current_block = res.result.current_block;
                                                app.update_tx_history(res.result.transactions, page, total_pages, current_block);
                                            }
                                        }
                                        Err(e) => {
                                            if let Ok(mut app) = app_state.write() {
                                                app.set_tx_history_error(format!("Failed to fetch transactions: {}", e));
                                            }
                                            if let Ok(mut log) = logger.write() {
                                                log.warn(format!("Failed to fetch tx history for {}: {}", ticker, e));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        KeyCode::Char('p') | KeyCode::Char('P') => {
                            // Previous page of transaction history
                            let ticker = app.selected_coin_ticker();
                            if ticker.is_some() {
                                app.tx_history_prev_page();
                                let page = app.tx_history_page();
                                drop(app);
                                if let Some(ticker) = ticker {
                                    // Fetch transaction history for new page
                                    if let Ok(mut app) = app_state.write() {
                                        app.set_tx_history_loading(true);
                                    }
                                    match kdf_client::my_tx_history(&rpc_password, &ticker, 10, Some(page)).await {
                                        Ok(res) => {
                                            if let Ok(mut app) = app_state.write() {
                                                let total_pages = res.result.total_pages;
                                                let page = res.result.paging_options.page_number;
                                                let current_block = res.result.current_block;
                                                app.update_tx_history(res.result.transactions, page, total_pages, current_block);
                                            }
                                        }
                                        Err(e) => {
                                            if let Ok(mut app) = app_state.write() {
                                                app.set_tx_history_error(format!("Failed to fetch transactions: {}", e));
                                            }
                                            if let Ok(mut log) = logger.write() {
                                                log.warn(format!("Failed to fetch tx history for {}: {}", ticker, e));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                }
                    }
                    _ => {}
                }
                if !crossterm::event::poll(zero)? {
                    break;
                }
            }
            if let Some((cols, rows)) = last_resize {
                let _ = terminal.resize(Rect::new(0, 0, cols, rows));
            }
        }

        // Update current time
        {
            if let Ok(mut app) = app_state.write() {
                app.update_current_time();
            }
        }

        // Poll one pending UTXO activation task per frame
        let to_poll: Option<(u64, String)> = app_state
            .read()
            .ok()
            .and_then(|a| a.pending_tasks().first().map(|(id, t)| (*id, t.clone())));
        if let Some((task_id, ticker)) = to_poll {
            match kdf_client::task_enable_utxo_status(&rpc_password, task_id, false).await {
                Ok(res) => {
                    let status = res.result.status;
                    let details = res.result.details.clone();
                    if let Ok(mut app) = app_state.write() {
                        app.remove_pending_task(task_id);
                        match status.as_str() {
                            "Ok" => {
                                if let Some(ref d) = coins::parse_status_details(&details, &ticker) {
                                    app.update_coin_from_status_details(&ticker, d);
                                }
                                app.update_coin_activated(&ticker);
                                if let Ok(mut log) = logger.write() {
                                    log.info(format!("Coin {} activated", ticker));
                                }
                            }
                            "InProgress" => {
                                app.add_pending_task(task_id, ticker.clone());
                                if let Ok(mut log) = logger.write() {
                                    let msg = details
                                        .as_str()
                                        .map(String::from)
                                        .unwrap_or_else(|| details.to_string());
                                    log.info(format!("{}: {}", ticker, msg));
                                }
                            }
                            _ => {
                                if let Ok(mut log) = logger.write() {
                                    log.warn(format!("{} task_id {} status: {}", ticker, task_id, status));
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    if let Ok(mut app) = app_state.write() {
                        app.remove_pending_task(task_id);
                    }
                    if let Ok(mut log) = logger.write() {
                        log.warn(format!("task::enable_utxo::status {} task_id {}: {}", ticker, task_id, e));
                    }
                }
            }
        }
        
        // Redraw to show any new log messages from background tasks
        terminal.draw(|f| {
            if let Ok(app) = app_state.read() {
                app.render(f);
            }
        })?;
        
        tokio::time::sleep(Duration::from_millis(16)).await;
    }
}
