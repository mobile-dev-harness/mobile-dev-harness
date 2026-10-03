//! Running a plan (ADR-0011): each cell's flows (or deep-linked screens) on its device and
//! configuration, compared with the reference cell, and a verdict per risk.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mdh_control::{Control, RunOptions as BuildOptions, Session};
use mdh_core::output::{Timings, millis};
use mdh_core::{Appearance, AppearanceKind, Device, Error, Result};
use mdh_driver::Driver;
use mdh_impact::ImpactReport;
use mdh_observe::UiTree;
use mdh_verify::{Flow, FlowOptions, FlowStore, Status, VerifyOptions, run_flow};
use mdh_visual::rules::{self, Rule};
use serde::Serialize;

use crate::plan::{Cell, Inventory, Plan, PlanOptions, Target, plan};
use mdh_risk::kb::Shape;
use mdh_risk::risk::{Dimension, Risk, risks};

/// Rules compared across cells: those on the tree that are never deliberate.
const RULES: [Rule; 4] = [
    Rule::TouchTarget,
    Rule::Label,
    Rule::Overlap,
    Rule::Obscured,
];
/// How long the UI gets to settle after a configuration change.
const SETTLE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub struct CompatOptions {
    /// Any directory inside the Gradle build, to build and install the app.
    pub project: PathBuf,
    pub base: String,
    pub flows_dir: PathBuf,
    pub runs_dir: PathBuf,
    /// The user agreed to start the plan's emulators.
    pub consent: bool,
    /// Leave out cells that need an emulator started (their risks stay unverified).
    pub no_start: bool,
    pub plan: PlanOptions,
    pub step_timeout: Duration,
    /// Build once and install on every device before its cells; off when the app is installed
    /// already (tests with scripted devices).
    pub install: bool,
}

impl Default for CompatOptions {
    fn default() -> Self {
        CompatOptions {
            project: PathBuf::from("."),
            base: "HEAD".into(),
            flows_dir: PathBuf::from(".mdh/flows"),
            runs_dir: PathBuf::from(".mdh/runs"),
            consent: false,
            no_start: false,
            plan: PlanOptions::default(),
            step_timeout: Duration::from_secs(10),
            install: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskStatus {
    Failed,
    Unverified,
    Passed,
}

#[derive(Debug, Clone, Serialize)]
pub struct RiskResult {
    #[serde(flatten)]
    pub risk: Risk,
    pub status: RiskStatus,
    /// What was found, per cell.
    pub notes: Vec<String>,
}

/// What ran on one cell.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CellRun {
    /// The cell couldn't run: no device, the install failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Flow (or `open <uri>`) → how it went.
    pub runs: BTreeMap<String, Outcome>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Outcome {
    pub passed: bool,
    /// The first failed check, for failures.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
    /// `rule element` → detail, on the screen the run ended on.
    pub violations: BTreeMap<String, String>,
    /// What a rotation lost.
    pub state_lost: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_dir: Option<PathBuf>,
    /// It couldn't run (a missing secret, an invalid flow): not a result either way.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub not_run: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompatReport {
    pub risks: Vec<RiskResult>,
    pub plan: Plan,
    pub cells: Vec<CellRun>,
    pub duration_ms: u64,
    pub text: String,
}

impl CompatReport {
    pub fn failure(&self) -> Option<Error> {
        let failed = self
            .risks
            .iter()
            .filter(|r| r.status == RiskStatus::Failed)
            .count();
        (failed > 0).then_some(Error::VerificationFailed {
            failed,
            total: self.risks.len(),
        })
    }
}

/// What can be run on: the session's device, the others online, the AVDs.
pub async fn inventory(session: &Session) -> Result<Inventory> {
    let driver = session.control().driver();
    let current = session.control().device().clone();
    let online = driver
        .devices()
        .await?
        .into_iter()
        .filter(|d| d.id != current.id && d.state == mdh_core::DeviceState::Online)
        .collect();
    let avds = driver.avds().await.unwrap_or_default();
    Ok(Inventory {
        current,
        online,
        avds,
    })
}

/// Impact, risks and the plan for the change, without running anything.
pub async fn prepare(
    session: &Session,
    options: &CompatOptions,
) -> Result<(ImpactReport, Vec<Risk>, Plan)> {
    let report = mdh_impact::analyze(&mdh_impact::Options {
        project: options.project.clone(),
        base: options.base.clone(),
    })
    .map_err(mdh_verify::impact_error)?;
    let risks = risks(&report);
    let inv = inventory(session).await?;
    let mut plan_options = options.plan;
    if options.no_start {
        plan_options.max_starts = 0;
    }
    let plan = plan(&risks, &inv, plan_options);
    Ok((report, risks, plan))
}

/// Verifies the change's risks: builds once, runs every cell of the plan, judges each risk.
pub async fn run(
    session: &mut Session,
    options: &CompatOptions,
    timings: &mut Timings,
) -> Result<CompatReport> {
    let started = Instant::now();
    let (report, risks, plan) = prepare(session, options).await?;
    if !plan.starts.is_empty() && !options.consent {
        return Err(Error::NeedsConsent {
            action: format!(
                "starting {} for the compatibility run",
                plan.starts
                    .iter()
                    .map(|a| format!("the emulator {a}"))
                    .collect::<Vec<_>>()
                    .join(" and ")
            ),
            retry: "run `mdh compat run --yes` (MCP: `consent: true`), or `--no-start` to leave those cells out".into(),
        });
    }
    let store = FlowStore::new(&options.flows_dir);
    let flows = load_flows(&store);
    let hosts: BTreeMap<String, String> = report
        .screens
        .iter()
        .filter_map(|s| s.host.clone().map(|h| (s.screen.clone(), h)))
        .collect();
    let links: BTreeMap<String, String> = report
        .screens
        .iter()
        .filter_map(|s| {
            s.reach
                .iter()
                .find(|r| r.contains("://"))
                .map(|r| (s.screen.clone(), r.clone()))
        })
        .collect();
    // What verifies each risk: the flows passing its screens, else their deep links.
    let per_risk: BTreeMap<String, Vec<Work>> = risks
        .iter()
        .map(|r| (r.id.clone(), work_for(r, &flows, &hosts, &links)))
        .collect();
    // What each cell runs: the work of its risks. The reference runs everything the other cells
    // on its device run, to compare with.
    let mut work: Vec<Vec<Work>> = plan
        .cells
        .iter()
        .map(|c| {
            let mut w: Vec<Work> = Vec::new();
            for item in c.risks.iter().filter_map(|id| per_risk.get(id)).flatten() {
                if !w.contains(item) {
                    w.push(item.clone());
                }
            }
            w
        })
        .collect();
    let reference_target = plan.cells[0].target.clone();
    let shared: Vec<Work> = work
        .iter()
        .zip(&plan.cells)
        .filter(|(_, c)| c.target == reference_target)
        .flat_map(|(w, _)| w.clone())
        .collect();
    for item in shared {
        if !work[0].contains(&item) {
            work[0].push(item);
        }
    }

    let package = session.app().map(str::to_owned);
    let driver: Arc<dyn Driver> = session.control().driver().clone();
    let mut runs: Vec<CellRun> = vec![CellRun::default(); plan.cells.len()];
    let mut built = false;
    let mut started_here: Vec<Device> = Vec::new();
    // Cells grouped by device, in plan order (the reference's device first).
    let mut targets: Vec<Target> = Vec::new();
    for c in &plan.cells {
        if !targets.contains(&c.target) {
            targets.push(c.target.clone());
        }
    }
    for target in targets {
        let indices: Vec<usize> = (0..plan.cells.len())
            .filter(|&i| plan.cells[i].target == target)
            .collect();
        let mut other;
        let device_session: &mut Session = if target == reference_target {
            &mut *session
        } else {
            let device = match &target {
                Target::Device { id, .. } => driver
                    .devices()
                    .await?
                    .into_iter()
                    .find(|d| &d.id == id)
                    .ok_or(Error::DeviceNotFound { id: id.clone() }),
                Target::Start { avd, .. } => {
                    let d = driver.start_emulator(avd, true).await;
                    if let Ok(d) = &d {
                        started_here.push(d.clone());
                    }
                    d
                }
            };
            match device {
                Ok(d) => {
                    other = Session::open(Control::new(driver.clone(), d), None);
                    &mut other
                }
                Err(e) => {
                    for &i in &indices {
                        runs[i].error = Some(e.to_string());
                    }
                    continue;
                }
            }
        };
        // Build once, install wherever it runs.
        let installed = if !options.install {
            Ok(())
        } else {
            device_session
                .run(
                    BuildOptions {
                        project: options.project.clone(),
                        module: None,
                        variant: None,
                        build: !built,
                        grant: false,
                        reinstall: false,
                    },
                    timings,
                    |_| {},
                )
                .await
                .and_then(|mut r| r.take_failure().map_or(Ok(()), Err))
        };
        built = true;
        if let Err(e) = installed {
            for &i in &indices {
                runs[i].error = Some(format!("couldn't install the app: {e}"));
            }
            continue;
        }
        for &i in &indices {
            let cell = &plan.cells[i];
            runs[i] = run_cell(
                device_session,
                cell,
                &work[i],
                package.as_deref(),
                options,
                timings,
            )
            .await;
        }
    }
    for d in started_here {
        let _ = driver.stop_emulator(&d).await;
    }

    let results = judge(&risks, &plan, &runs, &per_risk);
    let mut out = CompatReport {
        risks: results,
        plan,
        cells: runs,
        duration_ms: millis(started),
        text: String::new(),
    };
    out.text = crate::render::run(&out);
    Ok(out)
}

/// One thing a cell runs.
#[derive(Debug, Clone, PartialEq)]
pub enum Work {
    Flow(Flow),
    /// A screen without a flow, opened by its deep link.
    Open {
        screen: String,
        uri: String,
    },
}

impl Work {
    fn key(&self) -> String {
        match self {
            Work::Flow(f) => f.name.clone(),
            Work::Open { uri, .. } => format!("open {uri}"),
        }
    }
}

fn load_flows(store: &FlowStore) -> Vec<Flow> {
    store
        .names()
        .unwrap_or_default()
        .iter()
        .filter_map(|n| store.load(n).ok())
        .collect()
}

/// The flows passing the risk's screens (or the activities hosting them), else a deep link to
/// each screen.
fn work_for(
    risk: &Risk,
    flows: &[Flow],
    hosts: &BTreeMap<String, String>,
    links: &BTreeMap<String, String>,
) -> Vec<Work> {
    if risk.all_flows {
        return flows.iter().cloned().map(Work::Flow).collect();
    }
    let screens: BTreeSet<&str> = risk
        .screens
        .iter()
        .flat_map(|s| std::iter::once(s.as_str()).chain(hosts.get(s).map(String::as_str)))
        .collect();
    let mut out: Vec<Work> = flows
        .iter()
        .filter(|f| f.screens.iter().any(|s| screens.contains(s.as_str())))
        .cloned()
        .map(Work::Flow)
        .collect();
    for s in &risk.screens {
        let covered = out.iter().any(|w| match w {
            Work::Flow(f) => f.screens.iter().any(|x| x == s || hosts.get(s) == Some(x)),
            Work::Open { .. } => false,
        });
        if !covered && let Some(uri) = links.get(s) {
            out.push(Work::Open {
                screen: s.clone(),
                uri: uri.clone(),
            });
        }
    }
    out
}

async fn run_cell(
    session: &mut Session,
    cell: &Cell,
    work: &[Work],
    package: Option<&str>,
    options: &CompatOptions,
    timings: &mut Timings,
) -> CellRun {
    let mut out = CellRun::default();
    let restore = match apply(session, cell.shape).await {
        Ok(r) => r,
        Err(e) => {
            out.error = Some(format!("couldn't set up {}: {e}", cell.shape.describe()));
            return out;
        }
    };
    for item in work {
        let outcome = match item {
            Work::Flow(flow) => {
                let flow_options = FlowOptions {
                    verify: VerifyOptions {
                        runs: Some(options.runs_dir.clone()),
                        ..VerifyOptions::default()
                    },
                    step_timeout: options.step_timeout,
                };
                match run_flow(session, flow, &flow_options, timings).await {
                    Ok(v) => {
                        let failed: Vec<&mdh_verify::Finding> = v
                            .findings
                            .iter()
                            .filter(|f| f.outcome > mdh_verify::Outcome::Warn)
                            .collect();
                        // A crash explains the steps that failed after it.
                        let first = failed
                            .iter()
                            .find(|f| f.check == "no crash")
                            .or(failed.first());
                        Outcome {
                            passed: v.status == Status::Pass,
                            failure: first.map(|f| match &f.observed {
                                Some(o) => format!("{}: {o}", f.check),
                                None => f.check.clone(),
                            }),
                            run_dir: v.run_dir.clone(),
                            ..Outcome::default()
                        }
                    }
                    // Couldn't run at all (a missing secret, an invalid flow): nothing learned.
                    Err(e) => Outcome {
                        failure: Some(e.to_string()),
                        not_run: true,
                        ..Outcome::default()
                    },
                }
            }
            Work::Open { uri, .. } => {
                let since = session.device_time().await.ok();
                match session.open_uri(uri, package).await {
                    Ok(_) => {
                        tokio::time::sleep(SETTLE).await;
                        let crashes = session.app_crashes(since).await.unwrap_or_default();
                        Outcome {
                            passed: crashes.is_empty(),
                            failure: crashes.first().map(|c| format!("crashed: {}", c.summary)),
                            ..Outcome::default()
                        }
                    }
                    Err(e) => Outcome {
                        failure: Some(e.to_string()),
                        ..Outcome::default()
                    },
                }
            }
        };
        let mut outcome = outcome;
        if outcome.passed {
            if let Ok(v) = violations(session, timings).await {
                outcome.violations = v;
            }
            if cell.state_check {
                outcome.state_lost = state_check(session, timings).await;
            }
        }
        out.runs.insert(item.key(), outcome);
    }
    if let Err(e) = restore_shape(session, restore).await {
        out.error = Some(format!("couldn't restore the display: {e}"));
    }
    out
}

/// What to put back after a cell.
struct Restore {
    display: Option<Appearance>,
    rotation: Option<Appearance>,
}

/// Puts the device in the cell's shape; returns what to restore.
async fn apply(session: &mut Session, shape: Shape) -> Result<Restore> {
    let control = session.control();
    let mut restore = Restore {
        display: None,
        rotation: None,
    };
    if let Some((w, h)) = shape.size_dp() {
        let panel = control.driver().physical_display(control.device()).await?;
        restore.display = Some(control.appearance(&AppearanceKind::Display).await?);
        control.set_appearance(&display_for(panel, w, h)).await?;
    }
    if shape == Shape::Landscape {
        restore.rotation = Some(control.appearance(&AppearanceKind::Rotation).await?);
        control
            .set_appearance(&Appearance::Rotation(Some(1)))
            .await?;
    }
    if shape != Shape::Default {
        tokio::time::sleep(SETTLE).await;
    }
    Ok(restore)
}

async fn restore_shape(session: &mut Session, restore: Restore) -> Result<()> {
    let control = session.control();
    for value in [restore.display, restore.rotation].into_iter().flatten() {
        control.set_appearance(&value).await?;
    }
    Ok(())
}

/// The display override that shows `w`×`h` dp on `panel`: the density goes down until the size
/// fits the panel (in the same orientation), so screenshots stay within it.
pub fn display_for(panel: mdh_core::PhysicalDisplay, w: u32, h: u32) -> Appearance {
    let (pw, ph) = if (w > h) == (panel.width > panel.height) {
        (panel.width, panel.height)
    } else {
        (panel.height, panel.width)
    };
    let density = panel.density.min(pw * 160 / w).min(ph * 160 / h).max(80);
    Appearance::Display {
        size: Some((w * density / 160, h * density / 160)),
        density: Some(density),
    }
}

/// Rule violations on the current screen, keyed `rule element`.
async fn violations(
    session: &mut Session,
    timings: &mut Timings,
) -> Result<BTreeMap<String, String>> {
    let density = session.control().density().await?;
    let observed = session.observe(false, timings).await?;
    Ok(
        rules::check(&observed.tree, &observed.screen, density, &RULES)
            .into_iter()
            .map(|v| (format!("{} {}", v.rule.name(), v.element), v.detail))
            .collect(),
    )
}

/// Rotates to landscape and back; what the screen lost on the way: inputs, toggles and texts of
/// elements with an id, and elements with an id that are gone.
async fn state_check(session: &mut Session, timings: &mut Timings) -> Vec<String> {
    let Ok(before) = session.observe(false, timings).await else {
        return Vec::new();
    };
    let control = session.control();
    let Ok(original) = control.appearance(&AppearanceKind::Rotation).await else {
        return Vec::new();
    };
    let turned = control.set_appearance(&Appearance::Rotation(Some(1))).await;
    tokio::time::sleep(SETTLE).await;
    let back = session
        .control()
        .set_appearance(&Appearance::Rotation(Some(0)))
        .await;
    tokio::time::sleep(SETTLE).await;
    let _ = session.control().set_appearance(&original).await;
    if turned.is_err() || back.is_err() {
        return Vec::new();
    }
    let Ok(after) = session.observe(false, timings).await else {
        return Vec::new();
    };
    lost(&before.tree, &after.tree)
}

/// Elements with a unique id whose value, check state or text differ, or that are gone.
pub fn lost(before: &UiTree, after: &UiTree) -> Vec<String> {
    let by_id = |t: &UiTree| {
        let mut seen: BTreeMap<String, usize> = BTreeMap::new();
        for n in t.iter() {
            if let Some(id) = &n.id {
                *seen.entry(id.clone()).or_default() += 1;
            }
        }
        t.iter()
            .filter(|n| n.id.as_ref().is_some_and(|id| seen[id] == 1))
            .map(|n| (n.id.clone().unwrap_or_default(), n.clone()))
            .collect::<BTreeMap<_, _>>()
    };
    let (b, a) = (by_id(before), by_id(after));
    let mut out = Vec::new();
    for (id, old) in &b {
        let Some(new) = a.get(id) else {
            out.push(format!("#{id} is gone"));
            continue;
        };
        let show = |s: &Option<String>| format!("{:?}", s.as_deref().unwrap_or(""));
        if old.value != new.value && !old.state.password {
            out.push(format!(
                "#{id}: value {} → {}",
                show(&old.value),
                show(&new.value)
            ));
        }
        if old.state.checked != new.state.checked {
            out.push(format!(
                "#{id}: {} → {}",
                checked(old.state.checked),
                checked(new.state.checked)
            ));
        }
        if old.label != new.label && old.value.is_none() {
            out.push(format!(
                "#{id}: text {} → {}",
                show(&old.label),
                show(&new.label)
            ));
        }
    }
    out
}

fn checked(c: Option<bool>) -> &'static str {
    match c {
        Some(true) => "checked",
        Some(false) => "unchecked",
        None => "not checkable",
    }
}

/// A verdict per risk from what ran on its cells.
fn judge(
    risks: &[Risk],
    plan: &Plan,
    runs: &[CellRun],
    per_risk: &BTreeMap<String, Vec<Work>>,
) -> Vec<RiskResult> {
    let reference = &runs[0];
    let mut out = Vec::new();
    for risk in risks {
        let mut notes = Vec::new();
        let mut failed = false;
        let mut unverified = false;
        if let Some((_, why)) = plan.unverifiable.iter().find(|(id, _)| id == &risk.id) {
            notes.push(why.clone());
            unverified = true;
        }
        let cells: Vec<usize> = (0..plan.cells.len())
            .filter(|&i| plan.cells[i].risks.contains(&risk.id))
            .collect();
        let compare = matches!(
            risk.dimension,
            Dimension::ScreenSize | Dimension::DeviceType
        );
        for &i in &cells {
            let cell = &plan.cells[i];
            let run = &runs[i];
            if let Some(e) = &run.error {
                notes.push(format!("{}: {e}", cell.name()));
                unverified = true;
                continue;
            }
            let mine: Vec<String> = per_risk
                .get(&risk.id)
                .map(|w| w.iter().map(Work::key).collect())
                .unwrap_or_default();
            if mine.is_empty() {
                notes.push(format!(
                    "{}: no flow or deep link reaches {}; save a flow that does",
                    cell.name(),
                    if risk.screens.is_empty() {
                        "the changed code".to_owned()
                    } else {
                        risk.screens.join(", ")
                    }
                ));
                unverified = true;
                continue;
            }
            // Something has to have run on the cell for it to say anything.
            let ran = mine
                .iter()
                .any(|k| run.runs.get(k).is_some_and(|o| !o.not_run));
            if !ran {
                unverified = true;
            }
            for key in &mine {
                let Some(o) = run.runs.get(key) else { continue };
                let base = reference.runs.get(key);
                let name = cell.name();
                if o.not_run {
                    notes.push(format!(
                        "{name}: {key} couldn't run: {}",
                        o.failure.as_deref().unwrap_or("unknown")
                    ));
                    continue;
                }
                if !o.passed {
                    let what = o.failure.clone().unwrap_or_else(|| "failed".into());
                    if i == 0 {
                        notes.push(format!(
                            "{name}: {key} fails on the device as it is ({what}): a functional failure, fix it first"
                        ));
                        unverified = true;
                    } else if base.is_some_and(|b| !b.passed && !b.not_run) {
                        notes.push(format!(
                            "{name}: {key} fails on the reference too ({what}): not a compatibility difference"
                        ));
                        unverified = true;
                    } else {
                        notes.push(format!("{name}: {key}: {what}"));
                        failed = true;
                    }
                    continue;
                }
                if compare && i != 0 {
                    let known = base.map(|b| &b.violations);
                    for (k, detail) in &o.violations {
                        if known.is_some_and(|b| b.contains_key(k)) {
                            continue;
                        }
                        notes.push(format!(
                            "{name}: {key}: {} — {detail}",
                            k.split(' ').next().unwrap_or(k)
                        ));
                        failed = true;
                    }
                }
                for l in &o.state_lost {
                    notes.push(format!("{name}: {key}: rotation lost {l}"));
                    failed = true;
                }
            }
        }
        let status = if failed {
            RiskStatus::Failed
        } else if unverified || cells.is_empty() {
            RiskStatus::Unverified
        } else {
            RiskStatus::Passed
        };
        if status == RiskStatus::Passed {
            let on: Vec<String> = cells.iter().map(|&i| plan.cells[i].name()).collect();
            notes.push(format!("passed on {}", on.join("; ")));
        }
        out.push(RiskResult {
            risk: risk.clone(),
            status,
            notes,
        });
    }
    out.sort_by_key(|r| (r.status, r.risk.likelihood));
    out
}
