use std::fmt;
use std::io::{self, Write};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogSeverity {
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
    Core = 5,
}

impl LogSeverity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Error => "ERR",
            Self::Warn => "WRN",
            Self::Info => "INF",
            Self::Debug => "DBG",
            Self::Core => "COR",
        }
    }
}

impl fmt::Display for LogSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub severity: LogSeverity,
    pub process: Option<String>,
    pub elapsed: Duration,
    pub message: String,
}

struct LoggerState {
    started: Instant,
    console_level: LogSeverity,
    subscribers: Vec<Sender<LogEntry>>,
}

impl LoggerState {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            console_level: LogSeverity::Info,
            subscribers: Vec::new(),
        }
    }
}

static LOGGER: OnceLock<Mutex<LoggerState>> = OnceLock::new();

fn logger() -> &'static Mutex<LoggerState> {
    LOGGER.get_or_init(|| Mutex::new(LoggerState::new()))
}

pub fn level_log_set(level: usize) {
    let level = match level {
        0 | 1 => LogSeverity::Error,
        2 => LogSeverity::Warn,
        3 => LogSeverity::Info,
        4 => LogSeverity::Debug,
        _ => LogSeverity::Core,
    };
    logger().lock().unwrap().console_level = level;
}

pub fn level_log() -> LogSeverity {
    logger().lock().unwrap().console_level
}

pub fn subscribe_logs() -> Receiver<LogEntry> {
    let (tx, rx) = mpsc::channel();
    logger().lock().unwrap().subscribers.push(tx);
    rx
}

pub fn emit(severity: LogSeverity, process: Option<&str>, message: impl Into<String>) {
    let mut state = logger().lock().unwrap();
    let entry = LogEntry {
        severity,
        process: process.map(str::to_owned),
        elapsed: state.started.elapsed(),
        message: message.into(),
    };

    if severity <= state.console_level {
        let elapsed_ms = entry.elapsed.as_millis();
        let process = entry.process.as_deref().unwrap_or("-");
        let line = format!(
            "{elapsed_ms:>8}  {process:<24} {severity:<3}  {}",
            entry.message
        );
        if severity <= LogSeverity::Warn {
            let stream = &mut io::stderr();
            let _ = writeln!(stream, "{line}");
            let _ = stream.flush();
        } else {
            let stream = &mut io::stdout();
            let _ = writeln!(stream, "{line}");
            let _ = stream.flush();
        }
    }

    state
        .subscribers
        .retain(|subscriber| subscriber.send(entry.clone()).is_ok());
}

pub fn user_error(message: impl Into<String>) {
    emit(LogSeverity::Error, None, message);
}

pub fn user_warn(message: impl Into<String>) {
    emit(LogSeverity::Warn, None, message);
}

pub fn user_info(message: impl Into<String>) {
    emit(LogSeverity::Info, None, message);
}

pub fn user_debug(message: impl Into<String>) {
    emit(LogSeverity::Debug, None, message);
}
