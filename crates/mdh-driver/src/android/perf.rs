//! Parsers for what performance checks read: `dumpsys gfxinfo`, `dumpsys meminfo`,
//! `/proc/<pid>/stat`. Pure functions over captured text.

use mdh_core::{FrameStats, MemoryStats};

/// `dumpsys gfxinfo <pkg>`: frame counts and percentiles since the last reset.
pub fn parse_gfxinfo(out: &str) -> Option<FrameStats> {
    let mut f = FrameStats::default();
    let mut seen = false;
    for line in out.lines().map(str::trim) {
        let number = |rest: &str| {
            rest.trim()
                .trim_end_matches("ms")
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()
        };
        if let Some(rest) = line.strip_prefix("Total frames rendered:") {
            f.frames = number(rest)?;
            seen = true;
        } else if let Some(rest) = line.strip_prefix("Janky frames:") {
            f.janky = number(rest).unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("50th percentile:") {
            f.p50_ms = number(rest).unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("90th percentile:") {
            f.p90_ms = number(rest).unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("95th percentile:") {
            f.p95_ms = number(rest).unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("99th percentile:") {
            f.p99_ms = number(rest).unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("Number Slow UI thread:") {
            f.slow_ui_thread = number(rest).unwrap_or(0);
        }
    }
    seen.then_some(f)
}

/// `dumpsys meminfo <pkg>`: the App Summary in KB.
pub fn parse_meminfo(out: &str) -> Option<MemoryStats> {
    let field = |name: &str| {
        out.lines()
            .find_map(|l| l.trim().strip_prefix(name))
            .and_then(|rest| rest.split_whitespace().next()?.parse::<u64>().ok())
    };
    Some(MemoryStats {
        total_pss_kb: field("TOTAL PSS:")?,
        java_heap_kb: field("Java Heap:").unwrap_or(0),
        native_heap_kb: field("Native Heap:").unwrap_or(0),
        graphics_kb: field("Graphics:").unwrap_or(0),
    })
}

/// `/proc/<pid>/stat`: user plus system CPU time in clock ticks (fields 14 and 15, counted after
/// the parenthesized command, which may contain spaces).
pub fn parse_proc_stat_ticks(stat: &str) -> Option<u64> {
    let rest = &stat[stat.rfind(')')? + 1..];
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // After `)`: state is field 3, so utime (14) and stime (15) are at offsets 11 and 12.
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    Some(utime + stime)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/../../fixtures/android/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    #[test]
    fn gfxinfo() {
        let f = parse_gfxinfo(&fixture("dumpsys/gfxinfo_api36.txt")).unwrap();
        assert_eq!(f.frames, 46);
        assert_eq!(f.janky, 7);
        assert_eq!((f.p50_ms, f.p90_ms, f.p95_ms, f.p99_ms), (6, 48, 65, 150));
        assert_eq!(f.slow_ui_thread, 7);
        assert!((f.janky_pct() - 15.2).abs() < 0.1);
        assert_eq!(parse_gfxinfo("No process found for: x"), None);
    }

    #[test]
    fn meminfo() {
        let m = parse_meminfo(&fixture("dumpsys/meminfo_api36.txt")).unwrap();
        assert_eq!(m.total_pss_kb, 75586);
        assert_eq!(m.java_heap_kb, 13772);
        assert_eq!(m.native_heap_kb, 10508);
        assert_eq!(m.graphics_kb, 0);
    }

    #[test]
    fn proc_stat() {
        assert_eq!(
            parse_proc_stat_ticks(&fixture("dumpsys/proc_stat_api36.txt")),
            Some(207 + 83)
        );
        assert_eq!(
            parse_proc_stat_ticks("1 (a b) S 0 0 0 0 -1 0 0 0 0 0 5 6 0 0"),
            Some(11)
        );
    }
}
