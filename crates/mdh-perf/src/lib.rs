//! Perf: performance checks, a check kind of the verification engine (ADR-0009).
//!
//! Scope (functional design F11; milestone M6): cold/warm startup over repeated runs, frame timing
//! and jank (`dumpsys gfxinfo`), memory (`dumpsys meminfo`) including growth across repeated flows,
//! CPU sampling, budgets in config and comparison against stored baselines. Emulator numbers are
//! noisy, so results are relative to a baseline on the same device and report their variance.
//! Measurements and budget results land in a verdict; when something is slow, a Perfetto trace
//! says why ([`trace`]).

pub mod baseline;
pub mod metrics;
pub mod stats;
pub mod trace;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use mdh_control::{Session, new_run_dir};
use mdh_core::output::{Timings, millis};
use mdh_core::{Error, Input, Result};
use mdh_verify::{
    Check, CheckContext, CheckKind, Finding, Flow, FlowOptions, Outcome, Status, Verdict,
    VerifyOptions, run_flow,
};
use serde::Deserialize;

use crate::baseline::{Baseline, Store};
use crate::metrics::Metric;
use crate::stats::{Change, Summary, compare};

/// The `perf:` section of a flow, and what `mdh perf` is told.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerfConfig {
    /// Measured runs (one more is run first and discarded).
    #[serde(default)]
    pub runs: Option<usize>,
    /// Upper limits by metric key: `cold_start_ms`, `hot_start_ms`, `janky_pct`, `frame_p90_ms`,
    /// `frame_p99_ms`, `pss_mb`, `cpu_pct`. Judged on the median.
    #[serde(default)]
    pub budgets: BTreeMap<String, f64>,
}

impl PerfConfig {
    pub fn parse(value: Option<&serde_json::Value>) -> Result<PerfConfig> {
        match value {
            None => Ok(PerfConfig::default()),
            Some(v) => serde_json::from_value(v.clone()).map_err(|e| Error::InvalidFlow {
                flow: "perf".into(),
                reason: e.to_string(),
            }),
        }
    }

    fn budgets(&self) -> Result<Vec<(Metric, f64)>> {
        self.budgets
            .iter()
            .map(|(k, v)| {
                Metric::parse(k).map(|m| (m, *v)).ok_or_else(|| Error::InvalidFlow {
                    flow: "perf".into(),
                    reason: format!(
                        "unknown budget `{k}`; use cold_start_ms, hot_start_ms, janky_pct, frame_p90_ms, frame_p99_ms, pss_mb or cpu_pct"
                    ),
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct PerfOptions {
    pub runs: usize,
    /// Capture a Perfetto trace even if nothing regressed.
    pub trace: bool,
    /// Where evidence goes; `.mdh/runs`.
    pub runs_dir: Option<PathBuf>,
    pub baselines: PathBuf,
    /// Pause between runs, so one launch's aftermath (dex2oat, GC, disk) doesn't slow the next.
    pub cool_down: Duration,
}

impl Default for PerfOptions {
    fn default() -> Self {
        PerfOptions {
            runs: 5,
            trace: false,
            runs_dir: Some(PathBuf::from(".mdh/runs")),
            baselines: PathBuf::from(baseline::DIR),
            cool_down: Duration::from_millis(800),
        }
    }
}

/// Measurements of one scenario, ready to judge.
struct Measured {
    scope: String,
    package: String,
    samples: BTreeMap<Metric, Vec<f64>>,
    budgets: Vec<(Metric, f64)>,
    /// What a trace should explain if something is slow: startup or frames.
    explain: Explain,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Explain {
    Startup,
    Jank,
}

/// Cold (and with `hot`, hot) start of `app` over `options.runs` runs.
pub async fn startup(
    session: &mut Session,
    app: &str,
    hot: bool,
    options: &PerfOptions,
) -> Result<Verdict> {
    let started = Instant::now();
    let package = app.split_once('/').map_or(app, |(p, _)| p).to_owned();
    let restore = with_animations(session).await;
    let mut cold = Vec::new();
    // One discarded run first: it pays for the first launch after install (dex2oat, caches).
    for i in 0..=options.runs {
        session.control().stop(&package).await?;
        tokio::time::sleep(options.cool_down).await;
        let info = session.control().launch(app).await?;
        if i > 0 {
            cold.push(info.total_time_ms as f64);
        }
    }
    let mut samples = BTreeMap::from([(Metric::ColdStartMs, cold)]);
    if hot {
        let mut runs = Vec::new();
        for i in 0..=options.runs {
            session
                .control()
                .input(&Input::Key {
                    name: "HOME".into(),
                })
                .await?;
            tokio::time::sleep(options.cool_down).await;
            let info = session.control().launch(app).await?;
            if i > 0 {
                runs.push(info.total_time_ms as f64);
            }
        }
        samples.insert(Metric::HotStartMs, runs);
    }
    let measured = Measured {
        scope: format!("startup-{package}"),
        package: package.clone(),
        samples,
        budgets: Vec::new(),
        explain: Explain::Startup,
    };
    let verdict = judge(session, measured, options, started, app).await;
    if restore {
        let _ = session.animations(false).await;
    }
    verdict
}

/// Frames, memory and CPU while `flow` runs, over `options.runs` runs (budgets and runs from the
/// flow's `perf:` section). A run that fails functionally stops the measurement: its numbers
/// wouldn't mean anything.
pub async fn flow(
    session: &mut Session,
    flow: &Flow,
    options: &PerfOptions,
    timings: &mut Timings,
) -> Result<Verdict> {
    let started = Instant::now();
    let config = PerfConfig::parse(flow.perf.as_ref())?;
    let runs = config.runs.unwrap_or(options.runs).max(1);
    let package = flow
        .app
        .as_deref()
        .map(|a| a.split_once('/').map_or(a, |(p, _)| p).to_owned())
        .ok_or_else(|| Error::InvalidFlow {
            flow: flow.name.clone(),
            reason: "measuring a flow needs its `app`".into(),
        })?;
    // Jank can't be measured with animations off.
    let mut flow = flow.clone();
    flow.setup.animations = Some(true);
    let restore = with_animations(session).await;
    let sampler = Arc::new(Sampler::new(&package));
    let flow_options = FlowOptions {
        verify: VerifyOptions {
            runs: None,
            checks: vec![sampler.clone()],
            ..VerifyOptions::default()
        },
        ..FlowOptions::default()
    };
    for _ in 0..=runs {
        let verdict = run_flow(session, &flow, &flow_options, timings).await?;
        if verdict.status != Status::Pass {
            if restore {
                let _ = session.animations(false).await;
            }
            return Ok(Verdict::new(
                Some(format!("perf {}", flow.name)),
                verdict.findings,
                millis(started),
                None,
                Vec::new(),
            ));
        }
    }
    let mut samples = sampler.take();
    // The first run warms up caches and compiles code; it isn't representative.
    for values in samples.values_mut() {
        if !values.is_empty() {
            values.remove(0);
        }
    }
    let measured = Measured {
        scope: flow.name.clone(),
        package,
        samples,
        budgets: config.budgets()?,
        explain: Explain::Jank,
    };
    let verdict = judge_flow(
        session,
        measured,
        options,
        started,
        timings,
        &flow,
        &flow_options,
    )
    .await;
    if restore {
        let _ = session.animations(false).await;
    }
    verdict
}

/// Animations on for measuring (the session may have turned them off); true if they must be
/// turned off again afterwards.
async fn with_animations(session: &mut Session) -> bool {
    session.animations_off() && session.animations(true).await.is_ok()
}

/// Collects frame, memory and CPU numbers of each flow run: counters reset when the run begins,
/// read at its end.
struct Sampler {
    package: String,
    state: Mutex<SamplerState>,
}

#[derive(Default)]
struct SamplerState {
    started: Option<(Instant, u64, u32)>,
    samples: BTreeMap<Metric, Vec<f64>>,
}

impl Sampler {
    fn new(package: &str) -> Self {
        Sampler {
            package: package.to_owned(),
            state: Mutex::default(),
        }
    }

    fn take(&self) -> BTreeMap<Metric, Vec<f64>> {
        std::mem::take(&mut self.state.lock().expect("not poisoned").samples)
    }
}

#[async_trait]
impl Check for Sampler {
    fn kind(&self) -> CheckKind {
        CheckKind::Performance
    }

    async fn begin(&self, cx: &mut CheckContext<'_>) -> Result<()> {
        let control = cx.session.control();
        control
            .driver()
            .frame_stats(control.device(), &self.package, true)
            .await?;
        let pid = control
            .pids(std::slice::from_ref(&self.package))
            .await?
            .first()
            .copied()
            .unwrap_or(0);
        let cpu = if pid > 0 {
            control
                .driver()
                .cpu_time_ms(control.device(), pid)
                .await
                .unwrap_or(0)
        } else {
            0
        };
        self.state.lock().expect("not poisoned").started = Some((Instant::now(), cpu, pid));
        Ok(())
    }

    async fn run(&self, cx: &mut CheckContext<'_>) -> Result<Vec<Finding>> {
        if cx.checkpoint != "final" {
            return Ok(Vec::new());
        }
        let control = cx.session.control();
        let driver = control.driver();
        let frames = driver
            .frame_stats(control.device(), &self.package, false)
            .await?;
        let memory = driver.memory(control.device(), &self.package).await.ok();
        let started = self.state.lock().expect("not poisoned").started.take();
        let cpu = match started {
            Some((at, before, pid)) if pid > 0 => {
                let after = driver
                    .cpu_time_ms(control.device(), pid)
                    .await
                    .unwrap_or(before);
                let wall = at.elapsed().as_millis().max(1) as f64;
                Some((after.saturating_sub(before)) as f64 * 100.0 / wall)
            }
            _ => None,
        };
        let mut state = self.state.lock().expect("not poisoned");
        let mut push = |m: Metric, v: f64| state.samples.entry(m).or_default().push(v);
        if frames.frames > 0 {
            push(Metric::JankyPct, frames.janky_pct());
            push(Metric::FrameP90Ms, frames.p90_ms as f64);
            push(Metric::FrameP99Ms, frames.p99_ms as f64);
        }
        if let Some(m) = memory {
            push(Metric::PssMb, m.total_pss_kb as f64 / 1024.0);
        }
        if let Some(c) = cpu {
            push(Metric::CpuPct, c);
        }
        Ok(Vec::new())
    }
}

/// The device profile performance baselines are kept under: device, API level, build type.
async fn profile(session: &Session, package: &str) -> String {
    let device = session.control().device();
    let name = device
        .avd
        .as_deref()
        .or(device.model.as_deref())
        .unwrap_or(&device.id);
    let build = match session.control().driver().debuggable(device, package).await {
        Ok(true) => "debug",
        _ => "release",
    };
    let raw = format!("{name}-api{}-{build}", device.api.unwrap_or(0));
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-_.".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect()
}

async fn judge(
    session: &mut Session,
    m: Measured,
    options: &PerfOptions,
    started: Instant,
    app: &str,
) -> Result<Verdict> {
    let run_dir = options
        .runs_dir
        .as_deref()
        .map(|r| new_run_dir(r, "perf"))
        .transpose()?;
    let (mut findings, slow) = findings(session, &m, options).await?;
    if options.trace || slow {
        findings.push(
            trace_startup(
                session,
                &m.package,
                app,
                run_dir.as_deref(),
                options.cool_down,
            )
            .await,
        );
    }
    finish(&m, findings, started, run_dir)
}

async fn judge_flow(
    session: &mut Session,
    m: Measured,
    options: &PerfOptions,
    started: Instant,
    timings: &mut Timings,
    flow: &Flow,
    flow_options: &FlowOptions,
) -> Result<Verdict> {
    let run_dir = options
        .runs_dir
        .as_deref()
        .map(|r| new_run_dir(r, "perf"))
        .transpose()?;
    let (mut findings, slow) = findings(session, &m, options).await?;
    findings.extend(m.samples.get(&Metric::PssMb).and_then(|v| memory_growth(v)));
    if options.trace || slow {
        findings.push(
            trace_flow(
                session,
                &m.package,
                flow,
                flow_options,
                run_dir.as_deref(),
                timings,
            )
            .await,
        );
    }
    finish(&m, findings, started, run_dir)
}

fn finish(
    m: &Measured,
    findings: Vec<Finding>,
    started: Instant,
    run_dir: Option<PathBuf>,
) -> Result<Verdict> {
    let name = match m.explain {
        Explain::Startup => format!("perf startup {}", m.package),
        Explain::Jank => format!("perf {}", m.scope),
    };
    let verdict = Verdict::new(Some(name), findings, millis(started), run_dir, Vec::new());
    if let Some(dir) = &verdict.run_dir {
        let json = serde_json::to_vec_pretty(&verdict).expect("serializable");
        std::fs::write(dir.join("verdict.json"), json)?;
    }
    Ok(verdict)
}

/// One finding per metric (against the baseline and budgets), plus whether anything is slow
/// enough to explain.
async fn findings(
    session: &mut Session,
    m: &Measured,
    options: &PerfOptions,
) -> Result<(Vec<Finding>, bool)> {
    let profile = profile(session, &m.package).await;
    let store = Store {
        dir: options.baselines.clone(),
    };
    let path = store.path(&profile, &m.scope);
    let base = Store::load(&path);
    let mut out = Vec::new();
    let mut slow = false;
    let mut current = Baseline::default();
    for (metric, values) in &m.samples {
        let Some(now) = Summary::of(values) else {
            continue;
        };
        current.metrics.insert(*metric, now.clone());
        let budget = m.budgets.iter().find(|(b, _)| b == metric).map(|(_, v)| *v);
        let measured = format!(
            "{} median (p90 {}, ±{}, {} runs{})",
            metric.format(now.median),
            metric.format(now.p90),
            metric.format(now.mad),
            now.values.len(),
            budget.map_or(String::new(), |b| format!(", budget {}", metric.format(b)))
        );
        let check = format!("perf: {}", metric.name());
        let over_budget = budget.is_some_and(|b| now.median > b);
        let change = base.as_ref().and_then(|b| b.metrics.get(metric)).map(|b| {
            (
                compare(
                    &now,
                    b,
                    metric.minimum_change().0,
                    metric.minimum_change().1,
                ),
                b.clone(),
            )
        });
        let (outcome, observed) = match (&change, over_budget) {
            (Some((Change::Worse, b)), _) => {
                slow = true;
                let delta = now.median - b.median;
                (
                    Outcome::Fail,
                    format!(
                        "regressed: {} vs baseline {} (+{}, +{:.0}%) — {measured}",
                        metric.format(now.median),
                        metric.format(b.median),
                        metric.format(delta),
                        delta * 100.0 / b.median.max(f64::EPSILON)
                    ),
                )
            }
            (_, true) => {
                slow = true;
                (Outcome::Fail, format!("over budget: {measured}"))
            }
            (Some((Change::Better, b)), _) => (
                Outcome::Pass,
                format!(
                    "{measured}; better than baseline {} (`mdh perf approve` keeps it)",
                    metric.format(b.median)
                ),
            ),
            (Some((Change::Same, b)), _) => (
                Outcome::Pass,
                format!("{measured}; baseline {}", metric.format(b.median)),
            ),
            (None, false) if base.is_some() => (
                Outcome::Pass,
                format!("{measured}; not in the baseline yet (`mdh perf approve` adds it)"),
            ),
            (None, false) => (Outcome::Pass, measured),
        };
        out.push(Finding {
            kind: CheckKind::Performance,
            outcome,
            check,
            observed: Some(observed),
            step: None,
            evidence: Vec::new(),
        });
    }
    match &base {
        None => {
            Store::write(&path, &current)?;
            out.push(note(
                Outcome::Warn,
                format!("no baseline yet; recorded {} (commit it)", path.display()),
            ));
        }
        Some(_) => Store::write(&store.candidate(&profile, &m.scope), &current)?,
    }
    if profile.ends_with("-debug") {
        out.push(note(
            Outcome::Warn,
            "debug build: slower than release, so compare only with other debug builds".into(),
        ));
    }
    Ok((out, slow))
}

/// Memory that grows with every repetition of the same flow is a leak signal, whatever the
/// baseline says.
fn memory_growth(pss_mb: &[f64]) -> Option<Finding> {
    let (first, last) = (*pss_mb.first()?, *pss_mb.last()?);
    let grew = pss_mb.len() >= 3
        && pss_mb.windows(2).all(|w| w[1] >= w[0])
        && last - first >= (first * 0.1).max(5.0);
    grew.then(|| {
        note(
            Outcome::Warn,
            format!(
                "memory grew with every run: {first:.0} → {last:.0} MB over {} runs; something may be leaking",
                pss_mb.len()
            ),
        )
    })
}

fn note(outcome: Outcome, text: String) -> Finding {
    Finding {
        kind: CheckKind::Performance,
        outcome,
        check: "perf".into(),
        observed: Some(text),
        step: None,
        evidence: Vec::new(),
    }
}

/// A traced cold start, summarized; the trace file is kept as evidence.
async fn trace_startup(
    session: &mut Session,
    package: &str,
    app: &str,
    run_dir: Option<&Path>,
    cool_down: Duration,
) -> Finding {
    let captured: Result<PathBuf> = async {
        let control = session.control();
        control.stop(package).await?;
        tokio::time::sleep(cool_down).await;
        control
            .driver()
            .start_trace(control.device(), &trace::config(package, 30))
            .await?;
        let launched = control.launch(app).await;
        // Let the first frames and the post-launch work land in the trace.
        tokio::time::sleep(Duration::from_secs(2)).await;
        let bytes = control.driver().stop_trace(control.device()).await?;
        launched?;
        save_trace(&bytes, run_dir)
    }
    .await;
    explanation(captured, package, Scenario::Startup)
}

/// One more run of the flow under a trace, summarized.
async fn trace_flow(
    session: &mut Session,
    package: &str,
    flow: &Flow,
    flow_options: &FlowOptions,
    run_dir: Option<&Path>,
    timings: &mut Timings,
) -> Finding {
    let captured: Result<PathBuf> = async {
        {
            let control = session.control();
            control
                .driver()
                .start_trace(control.device(), &trace::config(package, 120))
                .await?;
        }
        let run = run_flow(session, flow, flow_options, timings).await;
        let control = session.control();
        let bytes = control.driver().stop_trace(control.device()).await?;
        run?;
        save_trace(&bytes, run_dir)
    }
    .await;
    explanation(captured, package, Scenario::Flow)
}

fn save_trace(bytes: &[u8], run_dir: Option<&Path>) -> Result<PathBuf> {
    let dir = run_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(std::env::temp_dir);
    let path = dir.join("trace.pftrace");
    std::fs::write(&path, bytes)?;
    Ok(path)
}

/// What a trace recorded, and so what to look for in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scenario {
    /// A cold start: the startup's main-thread work.
    Startup,
    /// A flow: late frames, and where the main thread spent its time.
    Flow,
}

/// Summarizes a trace the way `mdh perf` does when something is slow.
pub fn explain(trace: &Path, package: &str, scenario: Scenario) -> Result<Vec<String>> {
    let tp = match trace::locate() {
        Some(tp) => tp,
        None => {
            setup(false)?;
            unreachable!("setup without consent fails when nothing is installed")
        }
    };
    match scenario {
        Scenario::Startup => trace::explain_startup(&tp, trace, package),
        Scenario::Flow => {
            let mut lines = trace::explain_jank(&tp, trace, package)?;
            lines.extend(trace::explain_busy(&tp, trace, package)?);
            Ok(lines)
        }
    }
}

/// What the trace says, or why it says nothing. Without a trace processor the trace is still
/// kept, and the agent is told to ask the user before anything is downloaded.
fn explanation(captured: Result<PathBuf>, package: &str, scenario: Scenario) -> Finding {
    let path = match captured {
        Ok(p) => p,
        Err(e) => return note(Outcome::Warn, format!("no trace to explain it: {e}")),
    };
    let open = format!("trace: {} (open it in ui.perfetto.dev)", path.display());
    if trace::locate().is_none() {
        let startup = if scenario == Scenario::Startup {
            " --startup"
        } else {
            ""
        };
        return Finding {
            evidence: vec![open],
            ..note(
                Outcome::Warn,
                format!(
                    "trace not analyzed: Perfetto's trace processor isn't installed; ask the user \
                     whether to download it (about 14 MB), then `mdh perf setup --yes` and \
                     `mdh perf explain {}{startup} --app {package}`",
                    path.display()
                ),
            )
        };
    }
    match explain(&path, package, scenario) {
        Ok(mut lines) => {
            lines.push(open);
            Finding {
                kind: CheckKind::Performance,
                outcome: Outcome::Pass,
                check: "perf: why (Perfetto trace)".into(),
                observed: None,
                step: None,
                evidence: lines,
            }
        }
        Err(e) => Finding {
            evidence: vec![open],
            ..note(Outcome::Warn, format!("trace not analyzed: {e}"))
        },
    }
}

/// Makes sure Perfetto's trace processor, which explains slow traces, is there: where it is, or
/// with the user's `consent`, downloaded (the pinned version, checked against its SHA-256).
/// Without consent nothing is downloaded: the error tells the caller to ask.
pub fn setup(consent: bool) -> Result<String> {
    if let Some(path) = trace::locate() {
        return Ok(format!("trace processor: {}", path.display()));
    }
    if !consent {
        return Err(Error::NeedsConsent {
            action: format!(
                "downloading Perfetto's trace processor ({}, about 14 MB, from \
                 commondatastorage.googleapis.com)",
                trace::VERSION
            ),
            retry:
                "run `mdh perf setup --yes` (MCP: `mdh_perf` with `command: setup, consent: true`)"
                    .into(),
        });
    }
    let path = trace::download()?;
    Ok(format!(
        "downloaded the trace processor {} to {}",
        trace::VERSION,
        path.display()
    ))
}

/// Promotes the latest measurements to baselines (all, or one scope's).
pub fn approve(scope: Option<&str>) -> Result<String> {
    let store = Store {
        dir: PathBuf::from(baseline::DIR),
    };
    let approved = store.approve(scope)?;
    Ok(if approved.is_empty() {
        "no measurements to approve; run `mdh perf` first".to_owned()
    } else {
        let lines: Vec<String> = approved
            .iter()
            .map(|p| format!("  {}", p.display()))
            .collect();
        format!(
            "approved {} performance baseline(s):\n{}",
            approved.len(),
            lines.join("\n")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn growing_memory_is_a_leak_signal() {
        assert!(memory_growth(&[60.0, 64.0, 68.0, 72.0]).is_some());
        // Flat, noisy or too little: no signal.
        assert!(memory_growth(&[60.0, 61.0, 60.5, 61.0]).is_none());
        assert!(memory_growth(&[60.0, 70.0, 66.0, 72.0]).is_none());
        assert!(memory_growth(&[60.0, 61.0, 62.0, 63.0]).is_none());
        assert!(memory_growth(&[60.0, 80.0]).is_none());
    }

    #[test]
    fn budgets_are_checked_by_key() {
        let config = PerfConfig::parse(Some(&serde_json::json!({
            "runs": 3,
            "budgets": {"janky_pct": 5, "cold_start_ms": 800}
        })))
        .unwrap();
        assert_eq!(config.runs, Some(3));
        assert_eq!(
            config.budgets().unwrap(),
            [(Metric::ColdStartMs, 800.0), (Metric::JankyPct, 5.0)]
        );
        let unknown = PerfConfig::parse(Some(&serde_json::json!({"budgets": {"fps": 60}})))
            .unwrap()
            .budgets()
            .unwrap_err();
        assert!(
            unknown.to_string().contains("unknown budget `fps`"),
            "{unknown}"
        );
    }
}
