//! Parser for `logcat -v epoch -v uid` output.

use mdh_core::{LogEntry, LogLevel};

/// Parses lines like
/// `1790939484.936  1000 15922 15922 E AndroidRuntime: FATAL EXCEPTION: main`.
/// Buffer separators (`--------- beginning of crash`) and malformed lines are skipped.
pub fn parse_logcat(out: &str) -> Vec<LogEntry> {
    out.lines().filter_map(parse_line).collect()
}

fn parse_line(line: &str) -> Option<LogEntry> {
    let ([time, _uid, pid, tid, level], rest) = split_fields(line)?;
    let level = match level {
        "V" => LogLevel::Verbose,
        "D" => LogLevel::Debug,
        "I" => LogLevel::Info,
        "W" => LogLevel::Warn,
        "E" => LogLevel::Error,
        "F" | "A" => LogLevel::Fatal,
        _ => return None,
    };
    let (tag, message) = rest
        .split_once(": ")
        .unwrap_or((rest.trim_end_matches(':'), ""));
    Some(LogEntry {
        time_ms: parse_epoch_ms(time)?,
        pid: pid.parse().ok()?,
        tid: tid.parse().ok()?,
        level,
        tag: tag.trim().to_owned(),
        message: message.to_owned(),
    })
}

/// The first `N` whitespace-separated fields and the rest of the line after them, with the rest's
/// own spacing intact (stack frames in messages start with a tab).
fn split_fields<const N: usize>(line: &str) -> Option<([&str; N], &str)> {
    let mut fields = [""; N];
    let mut rest = line;
    for field in &mut fields {
        rest = rest.trim_start_matches(' ');
        let end = rest.find(' ').unwrap_or(rest.len());
        if end == 0 {
            return None;
        }
        (*field, rest) = rest.split_at(end);
    }
    Some((fields, rest.strip_prefix(' ').unwrap_or(rest)))
}

/// `1790939484.936` → 1790939484936.
fn parse_epoch_ms(s: &str) -> Option<u64> {
    let (secs, frac) = s.split_once('.')?;
    let mut millis: String = frac.chars().take(3).collect();
    while millis.len() < 3 {
        millis.push('0');
    }
    Some(secs.parse::<u64>().ok()? * 1000 + millis.parse::<u64>().ok()?)
}

/// `date +%s.%N` output → Unix milliseconds.
pub fn parse_device_time(out: &str) -> Option<u64> {
    parse_epoch_ms(out.trim())
}

/// `pidof` prints space-separated pids, or nothing (and exit status 1) when not running.
pub fn parse_pids(out: &str) -> Vec<u32> {
    out.split_whitespace()
        .filter_map(|p| p.parse().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_entries_and_keeps_message_spacing() {
        let out = "--------- beginning of crash
         1790939484.936  1000 15922 15922 E AndroidRuntime: FATAL EXCEPTION: main
         1790939484.936  1000 15922 15922 E AndroidRuntime: \tat android.os.Looper.loop(Looper.java:338)
         1790939484.937 media   584 15975 D ReflectedParamUpdater: extent() != 1: x
         garbage
";
        let entries = parse_logcat(out);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].time_ms, 1_790_939_484_936);
        assert_eq!(entries[0].pid, 15922);
        assert_eq!(entries[0].level, LogLevel::Error);
        assert_eq!(entries[0].tag, "AndroidRuntime");
        assert_eq!(entries[0].message, "FATAL EXCEPTION: main");
        assert_eq!(
            entries[1].message,
            "\tat android.os.Looper.loop(Looper.java:338)"
        );
        assert_eq!(entries[2].message, "extent() != 1: x");
    }

    #[test]
    fn parses_the_real_crash_capture() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/android/logcat/am_crash_settings_api36.txt"
        );
        let entries = parse_logcat(&std::fs::read_to_string(path).unwrap());
        assert!(entries.len() > 250, "{}", entries.len());
        assert!(
            entries
                .iter()
                .any(|e| e.message.starts_with("FATAL EXCEPTION"))
        );
    }

    #[test]
    fn device_time_and_pids() {
        assert_eq!(
            parse_device_time("1790939484.832512000\n"),
            Some(1_790_939_484_832)
        );
        assert_eq!(parse_pids("15922 16001\n"), [15922, 16001]);
        assert!(parse_pids("").is_empty());
    }
}
