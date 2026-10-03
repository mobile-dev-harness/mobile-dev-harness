//! What performance checks read from a device, platform-neutral.

use serde::{Deserialize, Serialize};

/// Frame timing since the counters were last reset.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FrameStats {
    pub frames: u64,
    /// Frames that missed their deadline.
    pub janky: u64,
    pub p50_ms: u64,
    pub p90_ms: u64,
    pub p95_ms: u64,
    pub p99_ms: u64,
    /// Frames slow because of the UI thread (as opposed to rendering or input).
    pub slow_ui_thread: u64,
}

impl FrameStats {
    pub fn janky_pct(&self) -> f64 {
        if self.frames == 0 {
            0.0
        } else {
            self.janky as f64 * 100.0 / self.frames as f64
        }
    }
}

/// An app's memory, in KB.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryStats {
    pub total_pss_kb: u64,
    pub java_heap_kb: u64,
    pub native_heap_kb: u64,
    pub graphics_kb: u64,
}
