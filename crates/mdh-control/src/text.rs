//! The compact text agents read for results that don't carry their own `text`. Shared by the CLI
//! and the MCP server.

use mdh_core::LaunchInfo;
use mdh_observe::{LogDigest, render_logs, render_screen};

use crate::{LogsReport, Screenshot, SessionSummary};

pub fn launch_text(info: &LaunchInfo) -> String {
    let mut s = format!(
        "launched {} ({} ms)",
        info.activity.as_deref().unwrap_or("?"),
        info.total_time_ms
    );
    if info.reused_existing {
        s.push_str(
            "\nwarning: an existing instance was brought to front; the app may not be on its start screen",
        );
    }
    s
}

impl LogsReport {
    pub fn text(&self) -> String {
        let mut out = vec![format!("logs of {}", self.packages.join(", "))];
        if !self.crashes.is_empty() {
            out.push(render_logs(&LogDigest {
                crashes: self.crashes.clone(),
                ..LogDigest::default()
            }));
        }
        if self.lines.is_empty() {
            out.push("(no matching lines in the last 10 minutes)".into());
        }
        out.extend(self.lines.iter().cloned());
        out.join("\n")
    }
}

impl SessionSummary {
    pub fn text(&self) -> String {
        let mut lines = vec![format!(
            "device {}  refs assigned: {}  steps: {}",
            self.device,
            self.refs_assigned,
            self.steps.len()
        )];
        if let Some(screen) = &self.screen {
            lines.push(format!("last {}", render_screen(screen)));
        }
        lines.extend(
            self.steps
                .iter()
                .enumerate()
                .map(|(i, s)| format!("{:>3}. {s}", i + 1)),
        );
        lines.join("\n")
    }
}

impl Screenshot {
    pub fn text(&self) -> String {
        format!(
            "{} ({}x{}, {} KB)",
            self.path.display(),
            self.image.width,
            self.image.height,
            self.image.bytes.len().div_ceil(1024)
        )
    }
}
