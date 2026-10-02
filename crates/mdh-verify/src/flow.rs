//! Flows: named, replayable steps and checks (functional design F7, §4.6), recorded from sessions
//! and replayed deterministically: every step waits for its target instead of sleeping.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mdh_control::{Action, Direction, RecordedStep, Session, Target, TextMatch};
use mdh_core::output::{Timings, millis};
use mdh_core::{Error, Result};
use mdh_observe::CrashKind;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::assertion::{Assertion, Element};
use crate::check::{Check, CheckContext, CheckKind, Finding, Outcome};
use crate::functional::Functional;
use crate::verdict::Verdict;
use crate::{VerifyOptions, collect_evidence, write_verdict, yaml};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Flow {
    pub name: String,
    /// The app under test; restarted before the steps unless `setup.launch` is false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(default, skip_serializing_if = "Setup::is_empty")]
    pub setup: Setup,
    /// Activities the flow passes (simple class names), recorded when it was saved; change impact
    /// uses them to pick the flows a change needs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub screens: Vec<String>,
    pub steps: Vec<Step>,
    /// Checked after the last step; `no crash` is always checked too.
    #[serde(default, rename = "assert", skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<FlowCheck>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Setup {
    /// `data` clears the app's data first: a first launch, logged out.
    #[serde(default, skip_serializing_if = "Reset::is_none")]
    pub reset: Reset,
    /// Runtime permissions to grant first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permissions: Vec<String>,
    /// Start from a deep link instead of the launcher activity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<String>,
    /// Restart the app before the steps (default); false continues from the current screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch: Option<bool>,
    /// System animations during the run: off by default (restored afterwards), `true` keeps them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animations: Option<bool>,
}

impl Setup {
    fn is_empty(&self) -> bool {
        *self == Setup::default()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reset {
    #[default]
    None,
    Data,
}

impl Reset {
    fn is_none(&self) -> bool {
        *self == Reset::None
    }
}

/// An assertion in a flow file: the inline form (`- enabled id=sign_in`) or the structured one
/// (`- enabled: { id: sign_in }`). Saved as the inline form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowCheck(pub Assertion);

impl Serialize for FlowCheck {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for FlowCheck {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Form {
            Inline(String),
            Structured(Assertion),
        }
        match Form::deserialize(d)? {
            Form::Inline(s) => Assertion::parse(&s)
                .map(FlowCheck)
                .map_err(serde::de::Error::custom),
            Form::Structured(a) => Ok(FlowCheck(a)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    /// Start an app (package or `package/activity`).
    Launch(String),
    /// Open a deep link.
    Open(String),
    Tap(Element),
    LongPress(Element),
    Type(TypeStep),
    Swipe(SwipeStep),
    Scroll(ScrollStep),
    /// A key such as `back`, `home`, `enter`.
    Key(String),
    Wait(WaitStep),
    /// Checks in the middle of a flow.
    Assert(Vec<FlowCheck>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypeStep {
    /// Supports `${env:NAME}`; secrets are recorded that way.
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub into: Option<Element>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub append: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub enter: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwipeStep {
    pub from: (i32, i32),
    pub to: (i32, i32),
    #[serde(default = "default_swipe_ms")]
    pub duration_ms: u32,
}

fn default_swipe_ms() -> u32 {
    300
}

/// `scroll: down`, or `scroll: { direction: down, until: "Row 30" }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScrollStep {
    pub direction: Direction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub within: Option<Element>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<Element>,
}

impl<'de> Deserialize<'de> for ScrollStep {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Full {
            direction: Direction,
            within: Option<Element>,
            until: Option<Element>,
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Form {
            Short(Direction),
            Full(Full),
        }
        Ok(match Form::deserialize(d)? {
            Form::Short(direction) => ScrollStep {
                direction,
                within: None,
                until: None,
            },
            Form::Full(f) => ScrollStep {
                direction: f.direction,
                within: f.within,
                until: f.until,
            },
        })
    }
}

/// `wait: id=inbox`, or `wait: { target: id=spinner, gone: true, timeout: 20 }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WaitStep {
    pub target: Element,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub gone: bool,
    /// Seconds; default: the flow's step timeout.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

impl<'de> Deserialize<'de> for WaitStep {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Full {
            target: Element,
            #[serde(default)]
            gone: bool,
            timeout: Option<u64>,
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Form {
            Full(Full),
            Short(Element),
        }
        Ok(match Form::deserialize(d)? {
            Form::Full(f) => WaitStep {
                target: f.target,
                gone: f.gone,
                timeout: f.timeout,
            },
            Form::Short(target) => WaitStep {
                target,
                gone: false,
                timeout: None,
            },
        })
    }
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Step::Launch(app) => write!(f, "launch {app}"),
            Step::Open(uri) => write!(f, "open {uri}"),
            Step::Tap(e) => write!(f, "tap {e}"),
            Step::LongPress(e) => write!(f, "long-press {e}"),
            Step::Type(t) => {
                write!(f, "type {:?}", t.text)?;
                if let Some(into) = &t.into {
                    write!(f, " into {into}")?;
                }
                Ok(())
            }
            Step::Swipe(s) => write!(f, "swipe {},{} → {},{}", s.from.0, s.from.1, s.to.0, s.to.1),
            Step::Scroll(s) => {
                write!(f, "scroll {}", format!("{:?}", s.direction).to_lowercase())?;
                if let Some(u) = &s.until {
                    write!(f, " until {u}")?;
                }
                Ok(())
            }
            Step::Key(k) => write!(f, "key {}", k.to_lowercase()),
            Step::Wait(w) => write!(f, "wait {}{}", w.target, if w.gone { " gone" } else { "" }),
            Step::Assert(checks) => write!(
                f,
                "assert {}",
                checks
                    .iter()
                    .map(|c| c.0.to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        }
    }
}

impl Flow {
    /// Environment variables the flow reads through `${env:NAME}`.
    pub fn secrets(&self) -> Vec<String> {
        let mut texts: Vec<&str> = self.setup.open.as_deref().into_iter().collect();
        for step in &self.steps {
            match step {
                Step::Type(t) => texts.push(&t.text),
                Step::Open(uri) => texts.push(uri),
                _ => {}
            }
        }
        let mut names = Vec::new();
        for t in texts {
            let mut rest = t;
            while let Some(start) = rest.find("${env:") {
                let after = &rest[start + 6..];
                let Some(end) = after.find('}') else { break };
                if !names.iter().any(|n| n == &after[..end]) {
                    names.push(after[..end].to_owned());
                }
                rest = &after[end + 1..];
            }
        }
        names
    }

    pub fn parse(name: &str, yaml_text: &str) -> Result<Flow> {
        yaml::from_str(name, yaml_text)
    }

    pub fn to_yaml(&self) -> String {
        yaml::to_string(self)
    }

    /// A flow from a session's recorded steps. Typed secrets become `${env:MDH_<FIELD>}`
    /// references; the names are returned so the caller can say which variables to set.
    pub fn from_recording(
        name: &str,
        app: Option<&str>,
        steps: &[RecordedStep],
        last_screen: Option<&str>,
        checks: Vec<Assertion>,
    ) -> (Flow, Vec<String>) {
        let mut screens: Vec<String> = Vec::new();
        for activity in steps
            .iter()
            .filter_map(|s| s.screen.as_deref())
            .chain(last_screen)
        {
            let simple = activity_name(activity).to_owned();
            if !screens.contains(&simple) {
                screens.push(simple);
            }
        }
        let mut secrets = Vec::new();
        let steps = steps
            .iter()
            .map(|s| match &s.action {
                Action::Tap { target } => Step::Tap(Element(target.clone())),
                Action::LongPress { target, .. } => Step::LongPress(Element(target.clone())),
                Action::Type {
                    text,
                    into,
                    append,
                    enter,
                } => {
                    let text = if text == "<secret>" {
                        let field = into
                            .as_ref()
                            .and_then(|t| match t {
                                Target::Selector(s) => s.id.clone().or_else(|| match &s.text {
                                    Some(TextMatch::Exact(t) | TextMatch::Label(t)) => {
                                        Some(t.clone())
                                    }
                                    _ => None,
                                }),
                                _ => None,
                            })
                            .unwrap_or_else(|| "secret".into());
                        let var = format!(
                            "MDH_{}",
                            field
                                .chars()
                                .map(|c| if c.is_ascii_alphanumeric() {
                                    c.to_ascii_uppercase()
                                } else {
                                    '_'
                                })
                                .collect::<String>()
                        );
                        if !secrets.contains(&var) {
                            secrets.push(var.clone());
                        }
                        format!("${{env:{var}}}")
                    } else {
                        text.clone()
                    };
                    Step::Type(TypeStep {
                        text,
                        into: into.clone().map(Element),
                        append: *append,
                        enter: *enter,
                    })
                }
                Action::Swipe {
                    from,
                    to,
                    duration_ms,
                } => Step::Swipe(SwipeStep {
                    from: *from,
                    to: *to,
                    duration_ms: *duration_ms,
                }),
                Action::Scroll {
                    direction,
                    within,
                    until,
                } => Step::Scroll(ScrollStep {
                    direction: *direction,
                    within: within.clone().map(Element),
                    until: until.clone().map(Element),
                }),
                Action::Key { name } => Step::Key(name.to_lowercase()),
            })
            .collect();
        let flow = Flow {
            name: name.to_owned(),
            app: app.map(str::to_owned),
            setup: Setup::default(),
            screens,
            steps,
            checks: checks.into_iter().map(FlowCheck).collect(),
        };
        (flow, secrets)
    }
}

/// Flows on disk: `<dir>/<name>.yaml`.
pub struct FlowStore {
    dir: PathBuf,
}

impl FlowStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        FlowStore { dir: dir.into() }
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.yaml"))
    }

    pub fn names(&self) -> Result<Vec<String>> {
        let mut names: Vec<String> = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries
                .flatten()
                .filter_map(|e| {
                    let p = e.path();
                    (p.extension().is_some_and(|x| x == "yaml" || x == "yml"))
                        .then(|| p.file_stem()?.to_str().map(str::to_owned))
                        .flatten()
                })
                .collect(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        names.sort();
        Ok(names)
    }

    pub fn load(&self, name: &str) -> Result<Flow> {
        let path = [self.path(name), self.dir.join(format!("{name}.yml"))]
            .into_iter()
            .find(|p| p.is_file())
            .ok_or_else(|| Error::FlowNotFound {
                name: name.to_owned(),
                available: self.names().unwrap_or_default(),
            })?;
        Flow::parse(name, &std::fs::read_to_string(path)?)
    }

    /// One line per flow: name, steps, checks, app.
    pub fn list(&self) -> Result<String> {
        let names = self.names()?;
        if names.is_empty() {
            return Ok(format!(
                "no flows in {}; act on the app, then save the recording as a flow",
                self.dir.display()
            ));
        }
        let lines: Vec<String> = names
            .iter()
            .map(|n| match self.load(n) {
                Ok(f) => format!(
                    "{n:<24} {} step{}, {} check{}{}",
                    f.steps.len(),
                    if f.steps.len() == 1 { "" } else { "s" },
                    f.checks.len(),
                    if f.checks.len() == 1 { "" } else { "s" },
                    f.app.map(|a| format!("  {a}")).unwrap_or_default()
                ),
                Err(e) => format!("{n:<24} invalid: {e}"),
            })
            .collect();
        Ok(lines.join("\n"))
    }

    pub fn save(&self, flow: &Flow) -> Result<PathBuf> {
        if flow.name.is_empty()
            || !flow
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(Error::InvalidFlow {
                flow: flow.name.clone(),
                reason: "names use letters, digits, `-` and `_`".into(),
            });
        }
        std::fs::create_dir_all(&self.dir)?;
        let path = self.path(&flow.name);
        std::fs::write(&path, flow.to_yaml())?;
        Ok(path)
    }
}

#[derive(Debug, Clone)]
pub struct FlowOptions {
    pub verify: VerifyOptions,
    /// How long each step waits for its target.
    pub step_timeout: Duration,
}

impl Default for FlowOptions {
    fn default() -> Self {
        FlowOptions {
            verify: VerifyOptions::default(),
            step_timeout: Duration::from_secs(10),
        }
    }
}

/// Replays `flow` and returns its verdict: the first step that can't run fails it; the remaining
/// steps are skipped; `no crash` is checked over the whole run either way.
pub async fn run_flow(
    session: &mut Session,
    flow: &Flow,
    options: &FlowOptions,
    timings: &mut Timings,
) -> Result<Verdict> {
    // Before touching the app: a missing secret would only fail halfway through.
    if let Some(name) = flow
        .secrets()
        .into_iter()
        .find(|n| std::env::var(n).is_err())
    {
        return Err(Error::MissingSecret { name });
    }
    let started = Instant::now();
    let since = session.device_time().await?;
    let run_dir = options
        .verify
        .runs
        .as_deref()
        .map(|r| mdh_control::new_run_dir(r, &format!("flow-{}", flow.name)))
        .transpose()?;
    let mut findings = Vec::new();
    let mut completed = 0;
    // Animations off for the run (best effort: not every device allows it), restored afterwards
    // unless the session had already turned them off.
    let restore_animations = flow.setup.animations != Some(true)
        && !session.animations_off()
        && session.animations(false).await.is_ok();

    let setup = setup(session, flow).await;
    let mut failed = setup.is_err();
    if let Err(e) = setup {
        findings.push(step_failure(None, "setup".into(), e.to_string()));
    }
    for (i, step) in flow.steps.iter().enumerate() {
        if failed {
            break;
        }
        let result = run_step(
            session,
            step,
            i,
            since,
            run_dir.as_deref(),
            options,
            timings,
        )
        .await?;
        failed = result.iter().any(|f| f.outcome >= Outcome::Fail);
        if !failed {
            completed += 1;
        }
        // A step reports findings only when it checks something or goes wrong.
        findings.extend(result);
    }
    let mut checks: Vec<Assertion> = if failed {
        Vec::new()
    } else {
        flow.checks.iter().map(|c| c.0.clone()).collect()
    };
    checks.push(Assertion::NoCrash);
    let mut cx = CheckContext {
        session,
        run_dir: run_dir.as_deref(),
        step: None,
        since_ms: Some(since),
        timings,
    };
    let finals = Functional {
        assertions: checks,
        timeout: options.verify.timeout,
    };
    findings.extend(finals.run(&mut cx).await?);

    let evidence = match &run_dir {
        Some(dir) => collect_evidence(cx.session, dir, cx.timings).await,
        None => Vec::new(),
    };
    if restore_animations {
        let _ = cx.session.animations(true).await;
    }
    let mut verdict = Verdict::new(
        Some(flow.name.clone()),
        findings,
        millis(started),
        run_dir,
        evidence,
    );
    verdict.set_steps(completed, flow.steps.len());
    write_verdict(&verdict)?;
    Ok(verdict)
}

async fn setup(session: &mut Session, flow: &Flow) -> Result<()> {
    let s = &flow.setup;
    let Some(app) = flow.app.as_deref() else {
        if let Some(uri) = &s.open {
            session.open_uri(&expand(uri)?, None).await?;
        }
        return Ok(());
    };
    let package = app.split_once('/').map_or(app, |(p, _)| p);
    if s.reset == Reset::Data {
        session.control().clear_data(package).await?;
    }
    for permission in &s.permissions {
        session
            .control()
            .set_permission(package, permission, true)
            .await?;
    }
    if let Some(uri) = &s.open {
        session.control().stop(package).await?;
        session.open_uri(&expand(uri)?, Some(package)).await?;
    } else if s.launch != Some(false) {
        session.control().stop(package).await?;
        session.launch(app).await?;
    }
    Ok(())
}

async fn run_step(
    session: &mut Session,
    step: &Step,
    index: usize,
    since: u64,
    run_dir: Option<&Path>,
    options: &FlowOptions,
    timings: &mut Timings,
) -> Result<Vec<Finding>> {
    let fail = |message: String| vec![step_failure(Some(index), step.to_string(), message)];
    // Steps aimed at an element first wait for it, so replay doesn't depend on timing.
    let target = match step {
        Step::Tap(e) | Step::LongPress(e) => Some(e),
        Step::Type(t) => t.into.as_ref(),
        Step::Scroll(s) => s.within.as_ref(),
        _ => None,
    };
    if let Some(e) = target
        && let Err(err) = session
            .wait(&e.0, false, options.step_timeout, timings)
            .await
    {
        let observed = match err {
            Error::Timeout { .. } => {
                let tree = session.observe(false, timings).await?;
                let activity = tree.screen.activity.as_deref();
                let visible = Assertion::Visible(e.clone()).evaluate(&tree.tree, activity);
                format!(
                    "{} after {} s (screen {})",
                    visible.observed.unwrap_or_else(|| "not on screen".into()),
                    options.step_timeout.as_secs(),
                    activity.map_or("unknown", |a| a.split_once('/').map_or(a, |(_, c)| c))
                )
            }
            other => other.to_string(),
        };
        return Ok(fail(observed));
    }
    let action = match step {
        Step::Launch(app) => {
            return Ok(match session.launch(app).await {
                Ok(_) => Vec::new(),
                Err(e) => fail(e.to_string()),
            });
        }
        Step::Open(uri) => {
            let package = session.app().map(str::to_owned);
            let result = match expand(uri) {
                Ok(uri) => session.open_uri(&uri, package.as_deref()).await.map(drop),
                Err(e) => Err(e),
            };
            return Ok(result
                .err()
                .map(|e| fail(e.to_string()))
                .unwrap_or_default());
        }
        Step::Wait(w) => {
            let timeout = w.timeout.map_or(options.step_timeout, Duration::from_secs);
            return Ok(
                match session.wait(&w.target.0, w.gone, timeout, timings).await {
                    Ok(_) => Vec::new(),
                    Err(e) => fail(e.to_string()),
                },
            );
        }
        Step::Assert(checks) => {
            let mut cx = CheckContext {
                session,
                run_dir,
                step: Some(index),
                since_ms: Some(since),
                timings,
            };
            let check = Functional {
                assertions: checks.iter().map(|c| c.0.clone()).collect(),
                timeout: options.verify.timeout,
            };
            return check.run(&mut cx).await;
        }
        Step::Tap(e) => Action::Tap {
            target: e.0.clone(),
        },
        Step::LongPress(e) => Action::LongPress {
            target: e.0.clone(),
            duration_ms: 800,
        },
        Step::Type(t) => match expand(&t.text) {
            Ok(text) => Action::Type {
                text,
                into: t.into.as_ref().map(|e| e.0.clone()),
                append: t.append,
                enter: t.enter,
            },
            Err(e) => return Ok(fail(e.to_string())),
        },
        Step::Swipe(s) => Action::Swipe {
            from: s.from,
            to: s.to,
            duration_ms: s.duration_ms,
        },
        Step::Scroll(s) => Action::Scroll {
            direction: s.direction,
            within: s.within.as_ref().map(|e| e.0.clone()),
            until: s.until.as_ref().map(|e| e.0.clone()),
        },
        Step::Key(k) => Action::Key {
            name: k.to_uppercase(),
        },
    };
    match session.act(action, timings).await {
        Ok(outcome) => {
            let crash = outcome.logs.as_ref().and_then(|l| {
                l.crashes
                    .iter()
                    .find(|c| c.of_app && c.kind != CrashKind::Died)
            });
            // The crash report itself comes with the `no crash` check.
            Ok(crash
                .map(|_| fail("the app crashed (report below)".into()))
                .unwrap_or_default())
        }
        Err(e) => Ok(fail(e.to_string())),
    }
}

/// `dev.app/.LoginActivity` → `LoginActivity`.
pub(crate) fn activity_name(component: &str) -> &str {
    let class = component.split_once('/').map_or(component, |(_, c)| c);
    class.rsplit('.').next().unwrap_or(class)
}

fn step_failure(step: Option<usize>, check: String, observed: String) -> Finding {
    Finding {
        kind: CheckKind::Functional,
        outcome: Outcome::Fail,
        check,
        observed: Some(observed),
        step,
        evidence: Vec::new(),
    }
}

/// Replaces `${env:NAME}` with the variable's value; a missing variable is an error, never an
/// empty string typed into a field.
fn expand(s: &str) -> Result<String> {
    expand_with(s, |name| std::env::var(name).ok())
}

fn expand_with(s: &str, var: impl Fn(&str) -> Option<String>) -> Result<String> {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find("${env:") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 6..];
        let end = after.find('}').ok_or_else(|| Error::InvalidFlow {
            flow: s.to_owned(),
            reason: "unterminated `${env:`".into(),
        })?;
        let name = &after[..end];
        let value = var(name).ok_or_else(|| Error::MissingSecret {
            name: name.to_owned(),
        })?;
        out.push_str(&value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOGIN: &str = r#"
name: login-success
app: dev.mdh.sample
setup:
  reset: data
  open: mdhsample://login
steps:
  - type: { into: id=email, text: alice@example.com }
  - type: { into: { id: password }, text: "${env:MDH_PASSWORD}" }
  - tap: id=sign_in
  - wait: { target: "Messages", timeout: 10 }
  - wait: role=progress
  - scroll: down
  - scroll: { direction: down, until: "Row 30" }
  - key: back
  - assert:
      - visible "Messages"
      - enabled: { id: sign_in }
assert:
  - screen .MessagesActivity
  - no crash
"#;

    #[test]
    fn parses_the_documented_format() {
        let flow = Flow::parse("login-success", LOGIN).unwrap();
        assert_eq!(flow.setup.reset, Reset::Data);
        assert_eq!(flow.steps.len(), 9);
        let shown: Vec<String> = flow.steps.iter().map(ToString::to_string).collect();
        assert_eq!(
            shown,
            [
                r#"type "alice@example.com" into id=email"#,
                r#"type "${env:MDH_PASSWORD}" into id=password"#,
                "tap id=sign_in",
                r#"wait "Messages""#,
                "wait role=progress",
                "scroll down",
                r#"scroll down until "Row 30""#,
                "key back",
                r#"assert visible "Messages"; enabled id=sign_in"#,
            ]
        );
        assert_eq!(flow.checks.len(), 2);
        // Saving and loading again gives the same flow.
        let again = Flow::parse("login-success", &flow.to_yaml()).unwrap();
        assert_eq!(again, flow);
    }

    #[test]
    fn errors_name_the_line() {
        let err = Flow::parse("x", "name: x\nsteps:\n  - tapp: Sign in\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("line 3"), "{err}");
    }

    #[test]
    fn recordings_become_flows_with_secret_references() {
        let step = |action| RecordedStep {
            at_ms: 0,
            action,
            description: String::new(),
            screen: Some("dev.mdh.sample/.MainActivity".into()),
        };
        let (flow, secrets) = Flow::from_recording(
            "login",
            Some("dev.mdh.sample"),
            &[
                step(Action::Tap {
                    target: Target::parse("Log in").unwrap(),
                }),
                step(Action::Type {
                    text: "<secret>".into(),
                    into: Some(Target::parse("id=password").unwrap()),
                    append: false,
                    enter: false,
                }),
                step(Action::Key {
                    name: "BACK".into(),
                }),
            ],
            Some("dev.mdh.sample/.MessagesActivity"),
            vec![Assertion::parse("screen .MessagesActivity").unwrap()],
        );
        assert_eq!(secrets, ["MDH_PASSWORD"]);
        let yaml = flow.to_yaml();
        assert_eq!(
            yaml,
            "name: login\napp: dev.mdh.sample\nscreens:\n- MainActivity\n- MessagesActivity\nsteps:\n- tap: Log in\n- type:\n    text: ${env:MDH_PASSWORD}\n    into: id=password\n- key: back\nassert:\n- screen .MessagesActivity\n"
        );
        assert_eq!(Flow::parse("login", &yaml).unwrap(), flow);
    }

    #[test]
    fn env_references_expand_or_fail() {
        let var = |name: &str| (name == "PW").then(|| "s3cret".to_owned());
        assert_eq!(expand_with("a ${env:PW} b", var).unwrap(), "a s3cret b");
        assert!(matches!(
            expand_with("${env:OTHER}", var),
            Err(Error::MissingSecret { .. })
        ));
    }
}
