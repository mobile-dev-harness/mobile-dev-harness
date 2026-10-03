//! What is measured, in which unit, and how big a change has to be to count.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    ColdStartMs,
    HotStartMs,
    JankyPct,
    FrameP90Ms,
    /// The slowest frames: a few very long ones (a stall while scrolling) barely move the share
    /// of janky frames or the p90.
    FrameP99Ms,
    PssMb,
    CpuPct,
}

impl Metric {
    pub fn name(self) -> &'static str {
        match self {
            Metric::ColdStartMs => "cold start",
            Metric::HotStartMs => "hot start",
            Metric::JankyPct => "janky frames",
            Metric::FrameP90Ms => "frame time p90",
            Metric::FrameP99Ms => "frame time p99",
            Metric::PssMb => "memory (PSS)",
            Metric::CpuPct => "CPU",
        }
    }

    /// The key in budgets and baselines.
    pub fn key(self) -> &'static str {
        match self {
            Metric::ColdStartMs => "cold_start_ms",
            Metric::HotStartMs => "hot_start_ms",
            Metric::JankyPct => "janky_pct",
            Metric::FrameP90Ms => "frame_p90_ms",
            Metric::FrameP99Ms => "frame_p99_ms",
            Metric::PssMb => "pss_mb",
            Metric::CpuPct => "cpu_pct",
        }
    }

    pub fn parse(key: &str) -> Option<Metric> {
        [
            Metric::ColdStartMs,
            Metric::HotStartMs,
            Metric::JankyPct,
            Metric::FrameP90Ms,
            Metric::FrameP99Ms,
            Metric::PssMb,
            Metric::CpuPct,
        ]
        .into_iter()
        .find(|m| m.key() == key)
    }

    /// `1,240 ms`, `7.2%`, `84 MB`.
    pub fn format(self, v: f64) -> String {
        match self {
            Metric::ColdStartMs | Metric::HotStartMs | Metric::FrameP90Ms | Metric::FrameP99Ms => {
                format!("{} ms", thousands(v.round() as i64))
            }
            Metric::JankyPct | Metric::CpuPct => format!("{v:.1}%"),
            Metric::PssMb => format!("{v:.0} MB"),
        }
    }

    /// The smallest absolute and relative change that counts, whatever the noise.
    pub fn minimum_change(self) -> (f64, f64) {
        match self {
            Metric::ColdStartMs => (50.0, 0.05),
            Metric::HotStartMs => (20.0, 0.10),
            Metric::JankyPct => (2.0, 0.0),
            Metric::FrameP90Ms => (4.0, 0.10),
            Metric::FrameP99Ms => (8.0, 0.25),
            Metric::PssMb => (5.0, 0.10),
            Metric::CpuPct => (5.0, 0.15),
        }
    }
}

fn thousands(v: i64) -> String {
    let s = v.abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if v < 0 { format!("-{out}") } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(Metric::ColdStartMs.format(1240.4), "1,240 ms");
        assert_eq!(Metric::JankyPct.format(7.25), "7.2%");
        assert_eq!(Metric::PssMb.format(84.4), "84 MB");
        assert_eq!(Metric::parse("janky_pct"), Some(Metric::JankyPct));
    }
}
