//! Log digests and crash reports (functional design F4.6–F4.8).
//!
//! Patterns are Android's (logcat tags `AndroidRuntime`, `DEBUG`, `ActivityManager`); other
//! platforms will add their own detectors behind the same types.

use std::collections::HashSet;

use mdh_core::{LogEntry, LogLevel};
use serde::{Deserialize, Serialize};

/// Most recent warning/error lines kept in a digest.
const RECENT: usize = 3;
/// Stack frames shown before folding the rest.
const FRAMES: usize = 6;
const MAX_MESSAGE_CHARS: usize = 160;
/// Graphics-stack and emulator chatter emitted inside every app process; never the app's own
/// problem. Left out of digests (still shown by `mdh logs`).
const NOISY_TAGS: &[&str] = &[
    "HWUI",
    "OpenGLRenderer",
    "EGL_emulation",
    "libEGL",
    "gralloc4",
    "Gralloc4",
    "AdrenoGLES",
    "vulkan",
    "RenderThread",
    "ion",
];
/// Digest lines are shorter: they ride along with every action, and `mdh logs` has the full text.
const RECENT_CHARS: usize = 100;

/// Which processes count as "the app" when filtering warnings and errors.
#[derive(Debug, Clone, Default)]
pub struct AppFilter {
    pub packages: Vec<String>,
    /// Known pids; pids announced in the logs (process starts, crashes) are added automatically.
    pub pids: HashSet<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LogDigest {
    pub errors: usize,
    pub warnings: usize,
    /// Latest warning and error lines of the app, `E/Tag: message`.
    pub recent: Vec<String>,
    /// Crashes, ANRs and unexpected deaths of any app.
    pub crashes: Vec<CrashReport>,
}

impl LogDigest {
    pub fn is_empty(&self) -> bool {
        self.errors == 0 && self.warnings == 0 && self.crashes.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrashKind {
    /// Uncaught Java/Kotlin exception.
    Java,
    /// Native crash (signal), reported by crash_dump.
    Native,
    /// Application not responding.
    Anr,
    /// The process died without a crash report (killed, low memory, …).
    Died,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashReport {
    pub kind: CrashKind,
    /// The crashed package is one of the app's (as opposed to some other process on the device).
    pub of_app: bool,
    pub package: Option<String>,
    pub pid: Option<u32>,
    pub time_ms: u64,
    /// `java.lang.IllegalStateException: boom`, `signal 11 (SIGSEGV) …`, the ANR reason, …
    pub summary: String,
    /// Shown frames: the app's own first, framework frames folded into `folded_frames`.
    pub frames: Vec<String>,
    pub folded_frames: usize,
    /// `Caused by:` chain, outermost first.
    pub caused_by: Vec<String>,
}

/// Digests `entries` (oldest first): counts and recent lines for the app, crash reports for all.
pub fn digest(entries: &[LogEntry], app: &AppFilter) -> LogDigest {
    let mut pids = app.pids.clone();
    for e in entries {
        if let Some(pid) = announced_pid(e, &app.packages) {
            pids.insert(pid);
        }
    }

    let mut d = LogDigest::default();
    let app_lines: Vec<&LogEntry> = entries
        .iter()
        .filter(|e| pids.contains(&e.pid) && e.level >= LogLevel::Warn)
        .filter(|e| e.tag != "AndroidRuntime") // reported as a crash instead
        .filter(|e| !NOISY_TAGS.contains(&e.tag.as_str()))
        // `Access denied finding property …` and the like; errors from libc still count.
        .filter(|e| !(e.tag == "libc" && e.level == LogLevel::Warn))
        .collect();
    d.errors = app_lines
        .iter()
        .filter(|e| e.level >= LogLevel::Error)
        .count();
    d.warnings = app_lines.len() - d.errors;
    d.recent = recent_distinct(&app_lines);
    d.crashes = crashes(entries, &app.packages);
    d
}

/// The latest distinct lines, newest last; repeats are counted instead of listed.
fn recent_distinct(lines: &[&LogEntry]) -> Vec<String> {
    let mut recent: Vec<(String, usize)> = Vec::new();
    for e in lines.iter().rev() {
        let line = format!(
            "{}/{}: {}",
            e.level.letter(),
            e.tag,
            truncate_to(e.message.trim(), RECENT_CHARS)
        );
        match recent.iter().position(|(l, _)| *l == line) {
            Some(i) => recent[i].1 += 1,
            None if recent.len() < RECENT => recent.push((line, 1)),
            None => {}
        }
    }
    recent
        .into_iter()
        .rev()
        .map(|(line, n)| {
            if n > 1 {
                format!("{line} (×{n})")
            } else {
                line
            }
        })
        .collect()
}

/// Pids of the app's processes from `Start proc 123:com.example/u0a1` and crash headers.
fn announced_pid(e: &LogEntry, packages: &[String]) -> Option<u32> {
    if e.tag == "ActivityManager" {
        let rest = e.message.strip_prefix("Start proc ")?;
        let (pid, rest) = rest.split_once(':')?;
        let package = rest.split(['/', ' ']).next()?;
        return packages
            .iter()
            .any(|p| p == package)
            .then(|| pid.parse().ok())?;
    }
    None
}

fn crashes(entries: &[LogEntry], app_packages: &[String]) -> Vec<CrashReport> {
    let mut reports = Vec::new();
    let mut i = 0;
    while i < entries.len() {
        let e = &entries[i];
        let (report, consumed) =
            if e.tag == "AndroidRuntime" && e.message.starts_with("FATAL EXCEPTION") {
                java_crash(&entries[i..], app_packages)
            } else if e.tag == "DEBUG" && e.message.starts_with("*** *** ***") {
                native_crash(&entries[i..], app_packages)
            } else if e.tag == "ActivityManager" && e.message.starts_with("ANR in ") {
                anr(&entries[i..])
            } else {
                (None, 1)
            };
        reports.extend(report);
        i += consumed.max(1);
    }
    // A death right after a crash is the crash itself; report only unexplained deaths.
    let explained: HashSet<u32> = reports.iter().filter_map(|r| r.pid).collect();
    for e in entries {
        if let Some((package, pid)) = death(e)
            && !explained.contains(&pid)
            && app_packages.contains(&package)
        {
            reports.push(CrashReport {
                kind: CrashKind::Died,
                of_app: true,
                package: Some(package),
                pid: Some(pid),
                time_ms: e.time_ms,
                summary: truncate(&e.message),
                frames: Vec::new(),
                folded_frames: 0,
                caused_by: Vec::new(),
            });
        }
    }
    for r in &mut reports {
        r.of_app = r.package.as_ref().is_some_and(|p| app_packages.contains(p));
    }
    reports.sort_by_key(|r| r.time_ms);
    reports
}

/// `AndroidRuntime` block of one process: header, `Process: pkg, PID: n`, exception, frames.
fn java_crash(entries: &[LogEntry], app_packages: &[String]) -> (Option<CrashReport>, usize) {
    let pid = entries[0].pid;
    let block: Vec<&LogEntry> = entries
        .iter()
        .take_while(|e| e.pid == pid && e.tag == "AndroidRuntime")
        .collect();
    let mut package = None;
    let mut summary = None;
    let mut frames = Vec::new();
    let mut caused_by = Vec::new();
    for e in &block[1..] {
        let m = e.message.trim();
        if let Some(rest) = m.strip_prefix("Process: ") {
            package = rest.split(',').next().map(str::to_owned);
        } else if let Some(frame) = m.strip_prefix("at ") {
            if caused_by.is_empty() {
                frames.push(frame.to_owned());
            }
        } else if let Some(cause) = m.strip_prefix("Caused by: ") {
            caused_by.push(truncate(cause));
        } else if summary.is_none() && !m.starts_with("...") && !m.is_empty() {
            summary = Some(truncate(m));
        }
    }
    let (frames, folded_frames) = fold_frames(
        frames,
        package
            .as_deref()
            .into_iter()
            .chain(app_packages.iter().map(String::as_str)),
    );
    let report = CrashReport {
        kind: CrashKind::Java,
        of_app: false,
        package,
        pid: Some(pid),
        time_ms: entries[0].time_ms,
        summary: summary.unwrap_or_else(|| "uncaught exception".into()),
        frames,
        folded_frames,
        caused_by,
    };
    (Some(report), block.len())
}

/// crash_dump's tombstone summary under the `DEBUG` tag.
fn native_crash(entries: &[LogEntry], app_packages: &[String]) -> (Option<CrashReport>, usize) {
    let pid_of_dump = entries[0].pid;
    let block: Vec<&LogEntry> = entries
        .iter()
        .take_while(|e| e.pid == pid_of_dump && e.tag == "DEBUG")
        .collect();
    let mut package = None;
    let mut pid = None;
    let mut signal = None;
    let mut frames = Vec::new();
    for e in &block {
        let m = e.message.trim();
        if m.starts_with("pid: ") {
            pid = m
                .strip_prefix("pid: ")
                .and_then(|r| r.split(',').next())
                .and_then(|p| p.trim().parse().ok());
            package = m
                .split_once(">>> ")
                .and_then(|(_, r)| r.split_once(" <<<"))
                .map(|(p, _)| p.to_owned());
        } else if m.starts_with("signal ") {
            signal = Some(truncate(m));
        } else if m.starts_with('#') {
            frames.push(m.to_owned());
        }
    }
    let (frames, folded_frames) = fold_frames(
        frames,
        package
            .as_deref()
            .into_iter()
            .chain(app_packages.iter().map(String::as_str)),
    );
    // App frames are recognized by their full path, so shorten only what is shown.
    let frames = frames.iter().map(|f| short_native_frame(f)).collect();
    let report = CrashReport {
        kind: CrashKind::Native,
        of_app: false,
        package,
        pid,
        time_ms: entries[0].time_ms,
        summary: signal.unwrap_or_else(|| "native crash".into()),
        frames,
        folded_frames,
        caused_by: Vec::new(),
    };
    (Some(report), block.len())
}

/// `#00 pc 00000000000d2608  /apex/…/bionic/libc.so (kill+8) (BuildId: 2445…)` → `#00 libc.so (kill+8)`.
/// Program counters and build ids mean nothing to an agent and cost ~60 characters per frame.
fn short_native_frame(frame: &str) -> String {
    let mut parts = frame.split_whitespace();
    let (Some(number), Some("pc"), Some(_pc), Some(path)) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return frame.to_owned();
    };
    let library = path.rsplit('/').next().unwrap_or(path);
    let symbol: Vec<&str> = parts.take_while(|p| !p.starts_with("(BuildId:")).collect();
    if symbol.is_empty() {
        format!("{number} {library}")
    } else {
        format!("{number} {library} {}", symbol.join(" "))
    }
}

/// `ANR in com.example (com.example/.MainActivity)` followed by `PID: n` and `Reason: …`.
fn anr(entries: &[LogEntry]) -> (Option<CrashReport>, usize) {
    let package = entries[0]
        .message
        .strip_prefix("ANR in ")
        .and_then(|r| r.split_whitespace().next())
        .map(str::to_owned);
    let mut pid = None;
    let mut reason = None;
    let mut consumed = 1;
    for e in entries.iter().skip(1).take(10) {
        if e.tag != "ActivityManager" {
            break;
        }
        consumed += 1;
        let m = e.message.trim();
        if let Some(p) = m.strip_prefix("PID: ") {
            pid = p.trim().parse().ok();
        } else if let Some(r) = m.strip_prefix("Reason: ") {
            reason = Some(truncate(r));
        }
    }
    let report = CrashReport {
        kind: CrashKind::Anr,
        of_app: false,
        package,
        pid,
        time_ms: entries[0].time_ms,
        summary: reason.unwrap_or_else(|| "application not responding".into()),
        frames: Vec::new(),
        folded_frames: 0,
        caused_by: Vec::new(),
    };
    (Some(report), consumed)
}

/// `Process com.example (pid 123) has died: …`.
fn death(e: &LogEntry) -> Option<(String, u32)> {
    if e.tag != "ActivityManager" {
        return None;
    }
    let rest = e.message.strip_prefix("Process ")?;
    let (package, rest) = rest.split_once(" (pid ")?;
    let (pid, rest) = rest.split_once(')')?;
    rest.trim_start()
        .starts_with("has died")
        .then(|| (package.to_owned(), pid.parse().ok()))
        .and_then(|(p, pid)| Some((p, pid?)))
}

/// Keeps the app's own frames (by package prefix) and the first frames, folding the rest.
fn fold_frames<'a>(
    frames: Vec<String>,
    packages: impl Iterator<Item = &'a str>,
) -> (Vec<String>, usize) {
    let packages: Vec<&str> = packages.collect();
    let is_app = |f: &str| {
        packages
            .iter()
            .any(|p| f.starts_with(p) || f.contains(&format!("/{p}")))
    };
    let app: Vec<&String> = frames.iter().filter(|f| is_app(f)).collect();
    let shown: Vec<String> = if app.is_empty() {
        frames.iter().take(FRAMES).cloned().collect()
    } else {
        app.into_iter().take(FRAMES).cloned().collect()
    };
    let folded = frames.len() - shown.len();
    (shown, folded)
}

fn truncate(s: &str) -> String {
    truncate_to(s, MAX_MESSAGE_CHARS)
}

fn truncate_to(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_owned(),
    }
}

/// The crash block and the warning/error line agents read; empty when there is nothing to say.
pub fn render_logs(d: &LogDigest) -> String {
    let mut lines = Vec::new();
    for c in &d.crashes {
        let kind = match c.kind {
            CrashKind::Java => "CRASH",
            CrashKind::Native => "NATIVE CRASH",
            CrashKind::Anr => "ANR",
            CrashKind::Died => "PROCESS DIED",
        };
        let who = match (&c.package, c.pid) {
            (Some(p), Some(pid)) => format!("{p} (pid {pid})"),
            (Some(p), None) => p.clone(),
            (None, Some(pid)) => format!("pid {pid}"),
            (None, None) => "unknown process".into(),
        };
        lines.push(format!("!! {kind} {who}: {}", c.summary));
        let prefix = if c.kind == CrashKind::Native {
            ""
        } else {
            "at "
        };
        lines.extend(c.frames.iter().map(|f| format!("     {prefix}{f}")));
        if c.folded_frames > 0 {
            lines.push(format!("     … {} more frames", c.folded_frames));
        }
        lines.extend(
            c.caused_by
                .iter()
                .map(|cause| format!("   caused by: {cause}")),
        );
    }
    if d.errors + d.warnings > 0 {
        let count = |n: usize, what: &str| match n {
            0 => None,
            1 => Some(format!("1 {what}")),
            n => Some(format!("{n} {what}s")),
        };
        let counts: Vec<String> = [count(d.errors, "error"), count(d.warnings, "warning")]
            .into_iter()
            .flatten()
            .collect();
        lines.push(format!(
            "logs: {} since last — {}",
            counts.join(", "),
            d.recent.join("; ")
        ));
    }
    lines.join("\n")
}
