//! Human-readable forms of library results. JSON output serializes the same values.

use mdh_control::{Observation, Screenshot};
use mdh_core::LaunchInfo;
use mdh_observe::render_screen;
use serde::Serialize;

use crate::output::Human;

/// Data of commands that only report success.
#[derive(Serialize)]
pub struct Done {
    pub done: String,
}

impl Done {
    pub fn new(done: impl Into<String>) -> Self {
        Self { done: done.into() }
    }
}

impl Human for Done {
    fn human(&self) -> String {
        self.done.clone()
    }
}

impl Human for Observation {
    fn human(&self) -> String {
        format!("{}\n{}", render_screen(&self.screen), self.text)
    }
}

impl Human for Screenshot {
    fn human(&self) -> String {
        format!(
            "{} ({}x{}, {} KB)",
            self.path.display(),
            self.image.width,
            self.image.height,
            self.image.bytes.len().div_ceil(1024)
        )
    }
}

impl Human for LaunchInfo {
    fn human(&self) -> String {
        let mut s = format!(
            "launched {} ({} ms)",
            self.activity.as_deref().unwrap_or("?"),
            self.total_time_ms
        );
        if self.reused_existing {
            s.push_str(
                "\nwarning: an existing instance was brought to front; the app may not be on its start screen",
            );
        }
        s
    }
}
