//! Verify: the verification engine every check runs in (ADR-0009).
//!
//! Scope (functional design F6, F7; milestone M4): flows (recorded from sessions, replayed
//! deterministically), the `Check` interface that check kinds implement, one verdict per run that
//! collects their findings with evidence (screenshots, tree excerpts, logs, numbers), the baseline
//! store, and reports (JUnit). Functional checks — assertions on screens and logs — are built in;
//! UI consistency (`mdh-visual`) and performance (`mdh-perf`) plug in as further check kinds.

pub mod agent;
mod assertion;
mod check;
mod flow;
mod functional;
mod junit;
mod verdict;
mod yaml;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use mdh_control::{Session, new_run_dir};
use mdh_core::output::{Timings, millis};
use mdh_core::{LogLevel, Result};

pub use assertion::{Assertion, Element, TextExpect};
pub use check::{Check, CheckContext, CheckKind, Finding, Outcome};
pub use flow::{
    Flow, FlowCheck, FlowOptions, FlowStore, Reset, ScrollStep, Setup, Step, SwipeStep, TypeStep,
    WaitStep, run_flow,
};
pub use functional::Functional;
pub use junit::junit;
pub use verdict::{Status, Verdict};

/// Longest edge of evidence screenshots.
const SCREENSHOT_EDGE: u32 = 1024;
/// Log lines kept as evidence.
const EVIDENCE_LOG_LINES: usize = 300;

#[derive(Clone)]
pub struct VerifyOptions {
    /// How long screen checks may take to start holding.
    pub timeout: Duration,
    /// `.mdh/runs`: where evidence is written; `None` keeps none.
    pub runs: Option<PathBuf>,
    /// Further check kinds (UI consistency, performance) run at every checkpoint, after the
    /// functional checks.
    pub checks: Vec<Arc<dyn Check>>,
    /// For a single verification: the name its baselines are kept under, and the check kinds'
    /// configuration (flows bring their own).
    pub scope: Option<String>,
    pub config: Option<serde_json::Value>,
}

impl std::fmt::Debug for VerifyOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerifyOptions")
            .field("timeout", &self.timeout)
            .field("runs", &self.runs)
            .field(
                "checks",
                &self.checks.iter().map(|c| c.kind()).collect::<Vec<_>>(),
            )
            .field("scope", &self.scope)
            .finish()
    }
}

impl Default for VerifyOptions {
    fn default() -> Self {
        VerifyOptions {
            timeout: Duration::from_secs(3),
            runs: Some(PathBuf::from(".mdh/runs")),
            checks: Vec::new(),
            scope: None,
            config: None,
        }
    }
}

/// Checks `assertions` on the app as it is now and returns the verdict. A crash of the app during
/// the session always fails it: `no crash` is added unless given.
pub async fn verify(
    session: &mut Session,
    mut assertions: Vec<Assertion>,
    options: &VerifyOptions,
    timings: &mut Timings,
) -> Result<Verdict> {
    let started = Instant::now();
    if !assertions.contains(&Assertion::NoCrash) {
        assertions.push(Assertion::NoCrash);
    }
    let mut checks: Vec<Arc<dyn Check>> = vec![Arc::new(Functional {
        assertions,
        timeout: options.timeout,
    })];
    checks.extend(options.checks.iter().cloned());
    let run_dir = options
        .runs
        .as_deref()
        .map(|r| new_run_dir(r, "verify"))
        .transpose()?;
    let mut findings = Vec::new();
    let mut cx = CheckContext {
        session,
        run_dir: run_dir.as_deref(),
        step: None,
        since_ms: None,
        scope: options.scope.as_deref(),
        checkpoint: "screen",
        config: options.config.as_ref(),
        timings,
    };
    for check in &checks {
        findings.extend(check.run(&mut cx).await?);
    }
    let evidence = match &run_dir {
        Some(dir) => collect_evidence(cx.session, dir, cx.timings).await,
        None => Vec::new(),
    };
    let verdict = Verdict::new(None, findings, millis(started), run_dir, evidence);
    write_verdict(&verdict)?;
    Ok(verdict)
}

pub(crate) fn write_verdict(verdict: &Verdict) -> Result<()> {
    if let Some(dir) = &verdict.run_dir {
        let json = serde_json::to_vec_pretty(verdict).expect("verdicts are serializable");
        std::fs::write(dir.join("verdict.json"), json)?;
    }
    Ok(())
}

/// Screenshot, tree and logs of the app as the verification left it. Evidence is best effort: a
/// screen that can't be captured doesn't change the verdict.
pub(crate) async fn collect_evidence(
    session: &mut Session,
    dir: &Path,
    timings: &mut Timings,
) -> Vec<String> {
    let mut written = Vec::new();
    if let Ok(image) = session.control().capture(SCREENSHOT_EDGE, timings).await
        && std::fs::write(dir.join("screenshot.jpg"), &image.bytes).is_ok()
    {
        written.push("screenshot.jpg".to_owned());
    }
    if let Ok(observed) = session.observe(false, timings).await
        && std::fs::write(dir.join("tree.txt"), &observed.text).is_ok()
    {
        written.push("tree.txt".to_owned());
    }
    if let Ok(logs) = session.logs(LogLevel::Info, EVIDENCE_LOG_LINES).await
        && std::fs::write(dir.join("logs.txt"), logs.text()).is_ok()
    {
        written.push("logs.txt".to_owned());
    }
    written
}

/// Several flows run one after another.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FlowRuns {
    pub verdicts: Vec<Verdict>,
    pub text: String,
}

impl FlowRuns {
    pub fn new(verdicts: Vec<Verdict>) -> Self {
        let mut parts: Vec<String> = verdicts.iter().map(|v| v.text.clone()).collect();
        if verdicts.len() > 1 {
            let passed = verdicts.iter().filter(|v| v.status == Status::Pass).count();
            parts.push(format!("flows: {passed} of {} passed", verdicts.len()));
        }
        FlowRuns {
            text: parts.join("\n\n"),
            verdicts,
        }
    }

    pub fn failure(&self) -> Option<mdh_core::Error> {
        let failed = self
            .verdicts
            .iter()
            .filter(|v| v.status != Status::Pass)
            .count();
        (failed > 0).then(|| mdh_core::Error::VerificationFailed {
            failed: self
                .verdicts
                .iter()
                .map(|v| v.findings.len() - v.passed())
                .sum(),
            total: self.verdicts.iter().map(|v| v.findings.len()).sum(),
        })
    }
}

/// A flow saved from a session's recording.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SavedFlow {
    pub path: PathBuf,
    pub steps: usize,
    pub checks: usize,
    /// Environment variables to set before replaying (typed secrets).
    pub secrets: Vec<String>,
    pub text: String,
}

/// Saves the session's recorded steps (the last `last` of them) as flow `name`, with `checks` run
/// after the last step.
pub fn save_flow(
    session: &Session,
    store: &FlowStore,
    name: &str,
    last: Option<usize>,
    checks: &[String],
    force: bool,
) -> Result<SavedFlow> {
    let invalid = |reason: &str| mdh_core::Error::InvalidFlow {
        flow: name.to_owned(),
        reason: reason.to_owned(),
    };
    if !force && store.path(name).exists() {
        return Err(invalid("already exists; overwrite it with force"));
    }
    let steps = session.steps();
    let steps = &steps[steps.len().saturating_sub(last.unwrap_or(steps.len()))..];
    if steps.is_empty() {
        return Err(invalid(
            "the session has no recorded steps; act on the app first",
        ));
    }
    let checks = checks
        .iter()
        .map(|c| Assertion::parse(c))
        .collect::<Result<Vec<_>>>()?;
    let (flow, secrets) = Flow::from_recording(
        name,
        session.app(),
        steps,
        session.current_activity(),
        checks,
    );
    let path = store.save(&flow)?;
    let plural = |n: usize, what: &str| format!("{n} {what}{}", if n == 1 { "" } else { "s" });
    let mut text = format!(
        "saved {} ({}, {})",
        path.display(),
        plural(flow.steps.len(), "step"),
        plural(flow.checks.len(), "check")
    );
    if flow.app.is_none() {
        text.push_str("; no app recorded, so it starts from whatever screen is showing");
    }
    if !secrets.is_empty() {
        text.push_str(&format!("; set {} before running it", secrets.join(", ")));
    }
    Ok(SavedFlow {
        path,
        steps: flow.steps.len(),
        checks: flow.checks.len(),
        secrets,
        text,
    })
}

/// Loads and replays flows one after another.
pub async fn run_flows(
    session: &mut Session,
    store: &FlowStore,
    names: &[String],
    options: &FlowOptions,
    timings: &mut Timings,
) -> Result<FlowRuns> {
    let flows = names
        .iter()
        .map(|n| store.load(n))
        .collect::<Result<Vec<_>>>()?;
    let mut verdicts = Vec::new();
    for flow in &flows {
        verdicts.push(run_flow(session, flow, options, timings).await?);
    }
    Ok(FlowRuns::new(verdicts))
}

/// Saved flows a change needs (functional design F14.6): those passing an affected screen or the
/// activity hosting it; all of them when the build configuration changed.
pub fn flows_for(report: &mdh_impact::ImpactReport, store: &FlowStore) -> Result<Vec<String>> {
    let build_changed = report.other_files.iter().any(|f| f.kind == "build");
    let affected: Vec<&str> = report
        .screens
        .iter()
        .flat_map(|s| std::iter::once(s.screen.as_str()).chain(s.host.as_deref()))
        .collect();
    let mut names = Vec::new();
    for name in store.names()? {
        let Ok(flow) = store.load(&name) else {
            continue;
        };
        if build_changed || flow.screens.iter().any(|s| affected.contains(&s.as_str())) {
            names.push(name);
        }
    }
    Ok(names)
}

/// Replays the saved flows the change since `base` needs (see [`flows_for`]).
pub async fn run_changed(
    session: &mut Session,
    store: &FlowStore,
    project: &Path,
    base: &str,
    options: &FlowOptions,
    timings: &mut Timings,
) -> Result<FlowRuns> {
    let report = mdh_impact::analyze(&mdh_impact::Options {
        project: project.to_owned(),
        base: base.to_owned(),
    })?;
    let names = flows_for(&report, store)?;
    if names.is_empty() {
        let screens: Vec<&str> = report.screens.iter().map(|s| s.screen.as_str()).collect();
        let text = if report.files_changed == 0 {
            format!("no changes against {base}; nothing to replay")
        } else if screens.is_empty() {
            "the change reaches no screen; no flow to replay (check it with `verify`)".to_owned()
        } else {
            format!(
                "no saved flow passes the affected screens ({}); check them with `verify`, or save a flow",
                screens.join(", ")
            )
        };
        return Ok(FlowRuns {
            verdicts: Vec::new(),
            text,
        });
    }
    let mut runs = run_flows(session, store, &names, options, timings).await?;
    runs.text = format!(
        "replaying {} for the change since {base}\n\n{}",
        names.join(", "),
        runs.text
    );
    Ok(runs)
}
