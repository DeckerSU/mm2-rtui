use chrono::Local;
use std::sync::{Arc, RwLock};
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
        }
    }
    
    pub fn color(&self) -> ratatui::style::Color {
        match self {
            LogLevel::Debug => ratatui::style::Color::Gray,
            LogLevel::Info => ratatui::style::Color::Cyan,
            LogLevel::Warn => ratatui::style::Color::Yellow,
            LogLevel::Error => ratatui::style::Color::Red,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: LogLevel,
    pub message: String,
}

pub struct Logger {
    entries: VecDeque<LogEntry>,
    max_entries: usize,
}

impl Logger {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(max_entries),
            max_entries,
        }
    }
    
    pub fn log(&mut self, level: LogLevel, message: String) {
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let entry = LogEntry {
            timestamp,
            level,
            message,
        };
        
        self.entries.push_back(entry);
        
        // Keep only the last max_entries
        while self.entries.len() > self.max_entries {
            self.entries.pop_front();
        }
    }
    
    pub fn debug(&mut self, message: String) {
        self.log(LogLevel::Debug, message);
    }
    
    pub fn info(&mut self, message: String) {
        self.log(LogLevel::Info, message);
    }
    
    pub fn warn(&mut self, message: String) {
        self.log(LogLevel::Warn, message);
    }
    
    pub fn error(&mut self, message: String) {
        self.log(LogLevel::Error, message);
    }
    
    pub fn get_entries(&self) -> &VecDeque<LogEntry> {
        &self.entries
    }
}

pub type SharedLogger = Arc<RwLock<Logger>>;

pub fn create_logger(max_entries: usize) -> SharedLogger {
    Arc::new(RwLock::new(Logger::new(max_entries)))
}
