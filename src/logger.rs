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

fn now_rfc3339() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let total_secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let sec = total_secs % 60;
    let min = (total_secs / 60) % 60;
    let hour = (total_secs / 3600) % 24;
    let days = total_secs / 86400;
    let (year, month, day) = days_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

/// Convert days-since-epoch to (year, month, day) in O(1).
/// Based on Howard Hinnant's `civil_from_days` algorithm.
#[allow(clippy::cast_sign_loss, clippy::unreadable_literal)]
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    let z = i64::try_from(days).unwrap_or(0) + 719_468;
    let era = if z >= 0 {
        z / 146_097
    } else {
        (z - 146_096) / 146_097
    };
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + 3 - 12 * (mp / 10);
    let y = y + mp / 10;
    (y as u64, m as u64, d as u64)
}

static LOGGER: OnceLock<SimpleLogger> = OnceLock::new();

pub fn init() {
    let level = parse_level(std::env::var("M2V_LOG_LEVEL").as_deref().unwrap_or("info"));
    let logger = LOGGER.get_or_init(|| SimpleLogger { max_level: level });
    let _ = log::set_logger(logger);
    log::set_max_level(level);
}

const fn parse_level(s: &str) -> LevelFilter {
    if s.eq_ignore_ascii_case("error") {
        LevelFilter::Error
    } else if s.eq_ignore_ascii_case("warn") {
        LevelFilter::Warn
    } else if s.eq_ignore_ascii_case("debug") {
        LevelFilter::Debug
    } else if s.eq_ignore_ascii_case("trace") {
        LevelFilter::Trace
    } else {
        LevelFilter::Info
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_level_exact() {
        assert_eq!(parse_level("error"), LevelFilter::Error);
        assert_eq!(parse_level("warn"), LevelFilter::Warn);
        assert_eq!(parse_level("info"), LevelFilter::Info);
        assert_eq!(parse_level("debug"), LevelFilter::Debug);
        assert_eq!(parse_level("trace"), LevelFilter::Trace);
    }

    #[test]
    fn parse_level_case_insensitive() {
        assert_eq!(parse_level("ERROR"), LevelFilter::Error);
        assert_eq!(parse_level("Trace"), LevelFilter::Trace);
    }

    #[test]
    fn parse_level_unknown_defaults_to_info() {
        assert_eq!(parse_level("banana"), LevelFilter::Info);
        assert_eq!(parse_level(""), LevelFilter::Info);
    }

    #[test]
    fn days_to_ymd_epoch() {
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
    }

    #[test]
    fn days_to_ymd_known_date() {
        // 2025-01-01 = 20089 days since epoch
        assert_eq!(days_to_ymd(20089), (2025, 1, 1));
    }

    #[test]
    fn days_to_ymd_leap_year() {
        // 2024-02-29 = 19782 days since epoch
        assert_eq!(days_to_ymd(19782), (2024, 2, 29));
    }
}
