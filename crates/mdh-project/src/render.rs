//! The compact build report agents read.

use std::fmt::Write;
use std::path::Path;

use crate::diagnostics::Severity;
use crate::gradle::BuildOutcome;

/// Errors shown in full; the rest are counted and left to the log.
const MAX_SHOWN: usize = 8;

/// `build :assembleDebug → failed (4.1 s), 2 errors` followed by `e: file:line:col message` lines
/// with the source line under each, and where the full log is.
pub fn render_build(b: &BuildOutcome, root: &Path) -> String {
    let secs = b.duration_ms as f64 / 1000.0;
    let mut out = if b.ok {
        format!(
            "build {} → {} ({secs:.1} s)",
            b.task,
            if b.up_to_date { "up to date" } else { "ok" }
        )
    } else {
        let mut s = format!("build {} → failed ({secs:.1} s)", b.task);
        if b.errors > 0 {
            let _ = write!(
                s,
                ", {} error{}",
                b.errors,
                if b.errors == 1 { "" } else { "s" }
            );
        }
        s
    };
    if b.warnings > 0 {
        let _ = write!(
            out,
            ", {} warning{}",
            b.warnings,
            if b.warnings == 1 { "" } else { "s" }
        );
    }
    let shown = b
        .diagnostics
        .iter()
        .filter(|d| !b.ok || d.severity == Severity::Error)
        .take(MAX_SHOWN);
    for d in shown {
        let letter = if d.severity == Severity::Error {
            'e'
        } else {
            'w'
        };
        let location = match (&d.file, d.line, d.column) {
            (Some(f), Some(l), Some(c)) => format!("{f}:{l}:{c} "),
            (Some(f), Some(l), None) => format!("{f}:{l} "),
            (Some(f), None, _) => format!("{f} "),
            (None, ..) => String::new(),
        };
        let _ = write!(out, "\n{letter}: {location}{}", d.message);
        if let (Some(source), Some(line)) = (&d.source, d.line) {
            let _ = write!(out, "\n   {line:>5} | {}", source.trim_end());
        }
        for note in &d.notes {
            let _ = write!(out, "\n     {note}");
        }
    }
    let hidden = b
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count()
        .saturating_sub(MAX_SHOWN);
    if hidden > 0 {
        let _ = write!(out, "\n… {hidden} more errors");
    }
    if !b.ok {
        let log = b.log.strip_prefix(root).unwrap_or(&b.log);
        let _ = write!(out, "\nfull log: {}", log.display());
    }
    out
}
