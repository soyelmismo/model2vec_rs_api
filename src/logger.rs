/// Minimal logger that writes to stderr, controlled by RUST_LOG env var.
/// Replaces tracing-subscriber with zero additional dependencies.
/// Supports: error, warn, info (default), debug, trace
use log::{LevelFilter, Log, Metadata, Record};
use std::sync::OnceLock;

struct SimpleLogger {
    max_level: LevelFilter,
}

impl Log for SimpleLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.max_level
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let level = record.level();
            let now = now_rfc3339();
            eprintln!("{now} {level:<5} {}", record.args());
        }
    }

    fn flush(&self) {}
}

/// ISO-8601 timestamp using only std — no chrono, no time crate.
fn now_rfc3339() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let total_secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    // Simple UTC formatting: YYYY-MM-DDTHH:MM:SSZ
    let sec = total_secs % 60;
    let min = (total_secs / 60) % 60;
    let hour = (total_secs / 3600) % 24;
    let days = total_secs / 86400; // days since epoch
    let (year, month, day) = days_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    let mut year = 1970u64;
    loop {
        let leap = is_leap(year);
        let dy = if leap { 366 } else { 365 };
        if days < dy {
            break;
        }
        days -= dy;
        year += 1;
    }
    let leap = is_leap(year);
    let months = if leap {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 1u64;
    for &dm in &months {
        if days < dm {
            break;
        }
        days -= dm;
        month += 1;
    }
    (year, month, days + 1)
}

fn is_leap(year: u64) -> bool {
    year.is_multiple_of(4) && !year.is_multiple_of(100) || year.is_multiple_of(400)
}

static LOGGER: OnceLock<SimpleLogger> = OnceLock::new();

pub fn init() {
    let level = parse_level(std::env::var("M2V_LOG_LEVEL").as_deref().unwrap_or("info"));
    let logger = LOGGER.get_or_init(|| SimpleLogger { max_level: level });
    let _ = log::set_logger(logger); // OnceLock guarantees single call — can't fail
    log::set_max_level(level);
}

fn parse_level(s: &str) -> LevelFilter {
    match s.to_ascii_lowercase().as_str() {
        "error" => LevelFilter::Error,
        "warn" => LevelFilter::Warn,
        "info" => LevelFilter::Info,
        "debug" => LevelFilter::Debug,
        "trace" => LevelFilter::Trace,
        _ => LevelFilter::Info,
    }
}
