//! Repeated measurements and when a difference is real: never one run, never a difference smaller
//! than the noise.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub median: f64,
    pub p90: f64,
    /// Median absolute deviation: the noise, robust to the odd outlier.
    pub mad: f64,
    pub values: Vec<f64>,
}

impl Summary {
    pub fn of(values: &[f64]) -> Option<Summary> {
        if values.is_empty() {
            return None;
        }
        let mut sorted = values.to_vec();
        sorted.sort_by(f64::total_cmp);
        let median = percentile(&sorted, 0.5);
        let mut deviations: Vec<f64> = sorted.iter().map(|v| (v - median).abs()).collect();
        deviations.sort_by(f64::total_cmp);
        Some(Summary {
            median,
            p90: percentile(&sorted, 0.9),
            mad: percentile(&deviations, 0.5),
            values: values.to_vec(),
        })
    }
}

/// Linear interpolation between closest ranks.
fn percentile(sorted: &[f64], q: f64) -> f64 {
    let pos = q * (sorted.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo as f64)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// Within the noise.
    Same,
    Worse,
    Better,
}

/// Compares a new summary with the baseline for a metric where lower is better. A change must
/// exceed three times the noise of either side, and the metric's minimum absolute and relative
/// difference, so emulator jitter doesn't fail verdicts.
pub fn compare(now: &Summary, base: &Summary, min_abs: f64, min_rel: f64) -> Change {
    let delta = now.median - base.median;
    let noise = 3.0 * now.mad.max(base.mad);
    let threshold = noise.max(min_abs).max(min_rel * base.median.abs());
    if delta > threshold {
        Change::Worse
    } else if -delta > threshold {
        Change::Better
    } else {
        Change::Same
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries() {
        let s = Summary::of(&[100.0, 110.0, 90.0, 105.0, 400.0]).unwrap();
        assert_eq!(s.median, 105.0);
        assert_eq!(s.mad, 5.0);
        assert!(s.p90 > 200.0);
        assert!(Summary::of(&[]).is_none());
    }

    #[test]
    fn noise_and_minimums_decide() {
        let base = Summary::of(&[1000.0, 1010.0, 990.0, 1005.0, 995.0]).unwrap();
        let same = Summary::of(&[1020.0, 1030.0, 1010.0, 1025.0, 1015.0]).unwrap();
        let worse = Summary::of(&[1180.0, 1190.0, 1175.0, 1185.0, 1200.0]).unwrap();
        let noisy = Summary::of(&[900.0, 1300.0, 1100.0, 1250.0, 950.0]).unwrap();
        assert_eq!(compare(&same, &base, 50.0, 0.05), Change::Same);
        assert_eq!(compare(&worse, &base, 50.0, 0.05), Change::Worse);
        assert_eq!(compare(&base, &worse, 50.0, 0.05), Change::Better);
        // A run this noisy can't show a 100 ms change.
        assert_eq!(compare(&noisy, &base, 50.0, 0.05), Change::Same);
    }
}
