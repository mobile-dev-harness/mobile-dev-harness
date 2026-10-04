//! Sessions: the state that makes consecutive calls behave like one conversation with the device.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mdh_core::output::{Timings, millis};
use mdh_core::ui::{Rect, ScreenInfo, TreeSource};
use mdh_core::{Error, Input, LaunchInfo, LogEntry, LogLevel, Result};
use mdh_observe::{
    AppFilter, CrashKind, CrashReport, LogDigest, RefTable, Role, TreeDiff, UiNode, UiTree, diff,
    digest, render, render_diff, render_line, render_logs, render_opaque, render_screen,
};
use serde::{Deserialize, Serialize};

use crate::action::{ActOutcome, Action, Direction};
use crate::settle::{SETTLE_TIMEOUT, settle};
use crate::target::{Resolved, Target, resolve, selector_for};
use crate::{Control, Snapshot};

/// Bump when the persisted shape changes; older files are discarded.
const STATE_VERSION: u32 = 2;
/// `scroll --until` stops when the content stops moving; this only bounds endless feeds. A small
/// fixed count made the result depend on the screen: 10 scrolls reached row 95 at 2992 px, not 2340.
const MAX_SCROLLS: usize = 50;
const FOCUS_TIMEOUT: Duration = Duration::from_secs(1);
const WAIT_POLL: Duration = Duration::from_millis(200);
const SCROLL_MS: u32 = 400;
/// Holding still before lifting stops lists where the finger stopped instead of flinging past the
/// content `scroll --until` is looking for (observed skipping 30+ rows).
const SCROLL_HOLD_MS: u32 = 150;
/// Steps listed under a crash, so the agent sees what led to it.
const STEPS_BEFORE_CRASH: usize = 3;
/// How far back `logs` looks, independent of the session's cursor.
const LOGS_WINDOW_MS: u64 = 10 * 60 * 1000;

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct State {
    version: u32,
    device: String,
    refs: RefTable,
    /// What the agent saw last; diffs are relative to it and stale refs are re-found through it.
    last: Option<View>,
    /// Ref → what it was and where it was last seen, to explain refs that are no longer on screen.
    #[serde(default)]
    seen: HashMap<String, String>,
    steps: Vec<RecordedStep>,
    /// Device time of the newest log entry already reported.
    #[serde(default)]
    pub(crate) log_cursor_ms: Option<u64>,
    /// Device time when the session started reading logs: the window `no_crash` checks.
    #[serde(default)]
    watching_since_ms: Option<u64>,
    /// The app the agent works on (set by `launch`); its logs and crashes are always watched,
    /// even when another app or the launcher is in front.
    #[serde(default)]
    app: Option<String>,
    /// The animation scales found before `animations(false)` turned them off; restored by
    /// `animations(true)` and on reset.
    #[serde(default)]
    saved_animations: Option<Vec<(String, Option<String>)>>,
    /// Package → what `run` last installed, to skip unchanged installs.
    #[serde(default)]
    pub(crate) installed: HashMap<String, Installed>,
}

/// An install made by `run`: the APK's hash and where it landed on the device.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Installed {
    pub(crate) apk_hash: u64,
    pub(crate) device_path: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct View {
    tree: UiTree,
    screen: ScreenInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedStep {
    /// Unix time in milliseconds.
    pub at_ms: u64,
    /// The action with refs replaced by selectors so it can be replayed in another session.
    /// Text typed into password fields is recorded as `<secret>`.
    pub action: Action,
    pub description: String,
    /// The activity the action was performed on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Observation {
    pub screen: ScreenInfo,
    pub source: TreeSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<TreeDiff>,
    pub tree: UiTree,
    /// Logs since the agent last looked: crashes, warning and error counts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logs: Option<LogDigest>,
    /// The compact text form agents read.
    pub text: String,
}

/// Recent logs of the app in front, independent of what was already reported.
#[derive(Debug, Serialize)]
pub struct LogsReport {
    /// Packages whose processes were included.
    pub packages: Vec<String>,
    /// `-12.3s E/Tag: message`, oldest first, relative to the device clock.
    pub lines: Vec<String>,
    pub crashes: Vec<CrashReport>,
}

#[derive(Debug, Serialize)]
pub struct SessionSummary {
    pub device: String,
    pub path: Option<PathBuf>,
    pub refs_assigned: usize,
    pub screen: Option<ScreenInfo>,
    pub steps: Vec<String>,
}

/// A [`Control`] plus session state. CLI invocations persist it to `path` between calls; the MCP
/// server keeps one in memory per connection.
pub struct Session {
    pub(crate) control: Control,
    pub(crate) state: State,
    path: Option<PathBuf>,
    /// Said once, at the top of the next observation: how the device was chosen or started.
    notice: Option<String>,
}

impl Session {
    /// Resumes the session stored at `path` if it belongs to the same device and version.
    pub fn open(control: Control, path: Option<PathBuf>) -> Self {
        let device = control.device().id.clone();
        let state = path
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|bytes| serde_json::from_slice::<State>(&bytes).ok())
            .filter(|s| s.version == STATE_VERSION && s.device == device)
            .unwrap_or_else(|| State {
                version: STATE_VERSION,
                device,
                ..State::default()
            });
        Self {
            control,
            state,
            path,
            notice: None,
        }
    }

    /// Something to tell the agent once, with the next observation or action.
    pub fn notify(&mut self, notice: impl Into<String>) {
        self.notice = Some(notice.into());
    }

    pub fn control(&self) -> &Control {
        &self.control
    }

    pub fn save(&self) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec(&self.state).expect("session state is serializable");
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Forgets refs, history and recording, and stops the device helper so other tools can use
    /// UiAutomation.
    pub async fn reset(&mut self) -> Result<()> {
        // Leave the device as it was found.
        if self.state.saved_animations.is_some() {
            let _ = self.animations(true).await;
        }
        self.state = State {
            version: STATE_VERSION,
            device: self.control.device().id.clone(),
            ..State::default()
        };
        if let Some(path) = &self.path {
            match std::fs::remove_file(path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
        }
        self.control.release().await
    }

    pub fn summary(&self) -> SessionSummary {
        SessionSummary {
            device: self.state.device.clone(),
            path: self.path.clone(),
            refs_assigned: self.state.refs.assigned(),
            screen: self.state.last.as_ref().map(|v| v.screen.clone()),
            steps: self
                .state
                .steps
                .iter()
                .map(|s| s.description.clone())
                .collect(),
        }
    }

    pub fn steps(&self) -> &[RecordedStep] {
        &self.state.steps
    }

    /// The current screen. With `diff_only`, only what changed since the agent last looked.
    pub async fn observe(&mut self, diff_only: bool, timings: &mut Timings) -> Result<Observation> {
        let started = Instant::now();
        let cursor = self.state.log_cursor_ms;
        let (snapshot, entries) = tokio::join!(self.control.snapshot(), self.logs_since(cursor));
        let snapshot = snapshot?;
        timings.record("observe", started);
        self.observation_from(snapshot, entries?, diff_only).await
    }

    /// Starts the log cursor now unless it runs already, so what happens next is reported.
    pub(crate) async fn start_log_cursor(&mut self) -> Result<()> {
        if self.state.log_cursor_ms.is_none() {
            let now = self.control.clock_ms().await?;
            self.state.log_cursor_ms = Some(now);
            self.state.watching_since_ms.get_or_insert(now);
        }
        Ok(())
    }

    /// Builds an observation from a snapshot and the log entries read with it.
    pub(crate) async fn observation_from(
        &mut self,
        snapshot: Snapshot,
        entries: Option<Vec<LogEntry>>,
        diff_only: bool,
    ) -> Result<Observation> {
        let source = snapshot.source;
        let view = self.adopt(snapshot);
        let logs = self.digest_logs(entries, &view).await?;
        let previous = if diff_only {
            self.state.last.as_ref()
        } else {
            None
        };
        let (diff, _, body) = report(previous, &view);
        let text = self.compose(None, &view, &body, logs.as_ref());
        self.state.last = Some(view.clone());
        Ok(Observation {
            screen: view.screen,
            source,
            diff,
            tree: view.tree,
            logs,
            text,
        })
    }

    /// Performs `action`, waits for the UI to settle and reports what changed since the agent
    /// last looked, including logs and crashes.
    pub async fn act(&mut self, action: Action, timings: &mut Timings) -> Result<ActOutcome> {
        let started = Instant::now();
        let snapshot = self.control.snapshot().await?;
        let before = self.adopt(snapshot);
        timings.record("observe_before", started);
        let previous = self.state.last.clone();

        let acted = Instant::now();
        let (description, recorded) = self
            .perform(&action, &before, previous.as_ref().map(|v| &v.tree))
            .await
            .map_err(|e| self.explain_stale_ref(e))?;
        timings.record("act", acted);

        let settling = Instant::now();
        let outcome = settle(&self.control, timings, Some(&before.tree)).await?;
        timings.record("settle", settling);
        let (settled, unresponsive_ms) = (outcome.settled, outcome.unresponsive_ms);
        let after = self.adopt(outcome.snapshot);
        self.state.steps.push(RecordedStep {
            at_ms: now_ms(),
            action: recorded,
            description: description.clone(),
            screen: before.screen.activity.clone(),
        });

        let reading = Instant::now();
        let entries = self.logs_since(self.state.log_cursor_ms).await?;
        let logs = self.digest_logs(entries, &before).await?;
        timings.record("logs", reading);

        let (diff, new_screen, body) = report(Some(previous.as_ref().unwrap_or(&before)), &after);
        let mut header = format!("{description} → ok ({} ms)", millis(started));
        if let Some(ms) = unresponsive_ms {
            header.push_str(&format!(
                " — the app did not respond: reading its UI took {:.1} s (main thread blocked? an ANR may follow)",
                ms as f64 / 1000.0
            ));
        } else if !settled {
            header.push_str(&format!(
                " — UI still changing after {} s",
                SETTLE_TIMEOUT.as_secs()
            ));
        }
        let text = self.compose(Some(&header), &after, &body, logs.as_ref());
        self.state.last = Some(after.clone());
        Ok(ActOutcome {
            action: description,
            settled,
            unresponsive_ms,
            new_screen,
            screen: after.screen,
            diff,
            tree: new_screen.then_some(after.tree),
            logs,
            text,
        })
    }

    /// Polls until `target` is on screen (or, with `gone`, no longer is).
    pub async fn wait(
        &mut self,
        target: &Target,
        gone: bool,
        timeout: Duration,
        timings: &mut Timings,
    ) -> Result<Observation> {
        let started = Instant::now();
        loop {
            let snapshot = self.control.snapshot().await?;
            let source = snapshot.source;
            let view = self.adopt(snapshot);
            let found = present(
                target,
                &view.tree,
                self.state.last.as_ref().map(|v| &v.tree),
            );
            if found != gone {
                timings.record("wait", started);
                let entries = self.logs_since(self.state.log_cursor_ms).await?;
                let logs = self.digest_logs(entries, &view).await?;
                let (diff, _, body) = report(self.state.last.as_ref(), &view);
                let header = format!(
                    "wait {target} → {} after {} ms",
                    if gone { "gone" } else { "found" },
                    millis(started),
                );
                let text = self.compose(Some(&header), &view, &body, logs.as_ref());
                self.state.last = Some(view.clone());
                return Ok(Observation {
                    screen: view.screen,
                    source,
                    diff,
                    tree: view.tree,
                    logs,
                    text,
                });
            }
            if started.elapsed() >= timeout {
                self.state.last = Some(view);
                return Err(Error::Timeout {
                    what: format!("{target} to {}", if gone { "disappear" } else { "appear" }),
                    seconds: timeout.as_secs(),
                });
            }
            tokio::time::sleep(WAIT_POLL).await;
        }
    }

    /// Launches `app` (package or component) and makes it the session's app.
    pub async fn launch(&mut self, app: &str) -> Result<LaunchInfo> {
        let info = self.control.launch(app).await?;
        let package = app.split_once('/').map_or(app, |(package, _)| package);
        self.state.app = Some(package.to_owned());
        Ok(info)
    }

    /// Opens a deep link; with `package`, only that app may handle it and it becomes the
    /// session's app.
    pub async fn open_uri(&mut self, uri: &str, package: Option<&str>) -> Result<LaunchInfo> {
        let info = self.control.open_uri(uri, package).await?;
        if let Some(p) = package {
            self.state.app = Some(p.to_owned());
        }
        Ok(info)
    }

    /// Recent logs of the app in front (and of the last seen screen's app), at least `level`;
    /// independent of what the session already reported.
    pub async fn logs(&mut self, level: LogLevel, lines: usize) -> Result<LogsReport> {
        let now = self.control.clock_ms().await?;
        let since = now.saturating_sub(LOGS_WINDOW_MS);
        let (snapshot, entries) = tokio::join!(self.control.snapshot(), self.control.logs(since));
        let view = self.adopt(snapshot?);
        let entries = entries?;
        let mut screens = vec![&view.screen];
        screens.extend(self.state.last.as_ref().map(|v| &v.screen));
        let packages = packages_of(self.state.app.as_deref(), &screens);
        let filter = AppFilter {
            pids: self.control.pids(&packages).await?.into_iter().collect(),
            packages: packages.clone(),
        };
        let crashes = digest(&entries, &filter).crashes;
        let mut shown: Vec<String> = entries
            .iter()
            .filter(|e| filter.pids.contains(&e.pid) && e.level >= level)
            .map(|e| {
                format!(
                    "{:>7.1}s {}/{}: {}",
                    (e.time_ms as f64 - now as f64) / 1000.0,
                    e.level.letter(),
                    e.tag,
                    e.message.trim_end()
                )
            })
            .collect();
        shown.drain(..shown.len().saturating_sub(lines));
        Ok(LogsReport {
            packages,
            lines: shown,
            crashes,
        })
    }

    /// Turns system animations off (remembering their scales) or back to what they were. Off,
    /// screens settle sooner and transitions can't be caught midway.
    pub async fn animations(&mut self, enabled: bool) -> Result<String> {
        if enabled {
            let saved = self.state.saved_animations.take();
            let scales = match &saved {
                Some(s) => s.clone(),
                // Nothing to restore: normal speed.
                None => self
                    .control
                    .animation_scales()
                    .await?
                    .into_iter()
                    .map(|(name, _)| (name, Some("1".to_owned())))
                    .collect(),
            };
            if let Err(e) = self.control.set_animation_scales(&scales).await {
                self.state.saved_animations = saved;
                return Err(e);
            }
            return Ok(if saved.is_some() {
                "animations restored".into()
            } else {
                "animations on".into()
            });
        }
        if self.state.saved_animations.is_none() {
            self.state.saved_animations = Some(self.control.animation_scales().await?);
        }
        let off: Vec<(String, Option<String>)> = self
            .state
            .saved_animations
            .iter()
            .flatten()
            .map(|(name, _)| (name.clone(), Some("0".to_owned())))
            .collect();
        self.control.set_animation_scales(&off).await?;
        Ok(
            "animations off; `session reset` or turning them on restores the previous scales"
                .into(),
        )
    }

    /// The package to act on: `package`, else the session's app.
    pub fn package_or_app(&self, package: Option<&str>) -> Result<String> {
        package
            .or(self.state.app.as_deref())
            .map(str::to_owned)
            .ok_or_else(|| Error::AppNotFound {
                package: "(none given, and the session has no app yet: launch or run one)".into(),
            })
    }

    /// Clears the app's data (`package`, else the session's app): a first launch again.
    pub async fn clear_data(&mut self, package: Option<&str>) -> Result<String> {
        let package = self.package_or_app(package)?;
        self.control.clear_data(&package).await?;
        Ok(format!("cleared the data of {package}"))
    }

    /// Grants or revokes a runtime permission of `package`, else of the session's app.
    pub async fn set_permission(
        &mut self,
        package: Option<&str>,
        permission: &str,
        granted: bool,
    ) -> Result<String> {
        let package = self.package_or_app(package)?;
        let permission = if permission.contains('.') {
            permission.to_owned()
        } else {
            format!("android.permission.{permission}")
        };
        self.control
            .set_permission(&package, &permission, granted)
            .await?;
        Ok(format!(
            "{} {permission} {} {package}",
            if granted { "granted" } else { "revoked" },
            if granted { "to" } else { "from" }
        ))
    }

    /// The activity the agent last saw, `package/.Activity`.
    pub fn current_activity(&self) -> Option<&str> {
        self.state.last.as_ref()?.screen.activity.as_deref()
    }

    /// Animations were turned off by this session.
    pub fn animations_off(&self) -> bool {
        self.state.saved_animations.is_some()
    }

    /// The session's app (set by `launch` and `run`), whose logs and crashes are always watched.
    pub fn app(&self) -> Option<&str> {
        self.state.app.as_deref()
    }

    /// The device's clock, which log times are on.
    pub async fn device_time(&self) -> Result<u64> {
        self.control.clock_ms().await
    }

    /// Crashes and ANRs of the app since `since_ms` (device time), whether or not they were
    /// already reported; by default since the session started watching. What `no_crash` checks:
    /// a crash the agent was shown and moved past still counts.
    pub async fn app_crashes(&mut self, since_ms: Option<u64>) -> Result<Vec<CrashReport>> {
        let now = self.control.clock_ms().await?;
        let since = since_ms
            .or(self.state.watching_since_ms)
            .unwrap_or_else(|| now.saturating_sub(LOGS_WINDOW_MS));
        let entries = self.control.logs(since).await?;
        let screens: Vec<&ScreenInfo> = self
            .state
            .last
            .as_ref()
            .map(|v| &v.screen)
            .into_iter()
            .collect();
        let packages = packages_of(self.state.app.as_deref(), &screens);
        let filter = AppFilter {
            pids: self.control.pids(&packages).await?.into_iter().collect(),
            packages,
        };
        Ok(digest(&entries, &filter)
            .crashes
            .into_iter()
            .filter(|c| c.of_app && c.kind != CrashKind::Died)
            .collect())
    }

    /// Entries after the cursor; `None` before the first read, which only starts the cursor so a
    /// session doesn't report the device's whole history.
    pub(crate) async fn logs_since(&self, cursor: Option<u64>) -> Result<Option<Vec<LogEntry>>> {
        match cursor {
            Some(since) => self.control.logs(since).await.map(Some),
            None => Ok(None),
        }
    }

    /// Digests new entries for the app the agent was working with (`before`, the last view) and
    /// advances the cursor. `None` when there's nothing to report.
    async fn digest_logs(
        &mut self,
        entries: Option<Vec<LogEntry>>,
        before: &View,
    ) -> Result<Option<LogDigest>> {
        let Some(entries) = entries else {
            let now = self.control.clock_ms().await?;
            self.state.log_cursor_ms = Some(now);
            self.state.watching_since_ms.get_or_insert(now);
            return Ok(None);
        };
        if let Some(newest) = entries.last() {
            self.state.log_cursor_ms = Some(newest.time_ms);
        }
        let mut screens = vec![&before.screen];
        screens.extend(self.state.last.as_ref().map(|v| &v.screen));
        let packages = packages_of(self.state.app.as_deref(), &screens);
        let filter = AppFilter {
            pids: self.control.pids(&packages).await?.into_iter().collect(),
            packages,
        };
        let d = digest(&entries, &filter);
        Ok((!d.is_empty()).then_some(d))
    }

    /// Header, screen line, crash block with the steps that led to it, body, log line.
    fn compose(
        &mut self,
        header: Option<&str>,
        view: &View,
        body: &str,
        logs: Option<&LogDigest>,
    ) -> String {
        let mut parts: Vec<String> = self
            .notice
            .take()
            .map(|n| format!("note: {n}"))
            .into_iter()
            .collect();
        parts.extend(header.map(str::to_owned));
        parts.push(render_screen(&view.screen));
        let rendered = logs.map(render_logs).unwrap_or_default();
        let (log_line, crash): (Vec<&str>, Vec<&str>) =
            rendered.lines().partition(|l| l.starts_with("logs: "));
        if !crash.is_empty() {
            parts.push(crash.join("\n"));
            let recent: Vec<&str> = self
                .state
                .steps
                .iter()
                .rev()
                .take(STEPS_BEFORE_CRASH)
                .rev()
                .map(|s| s.description.as_str())
                .collect();
            if !recent.is_empty() {
                parts.push(format!("   after: {}", recent.join(" → ")));
            }
        }
        parts.push(body.to_owned());
        parts.extend(log_line.into_iter().map(str::to_owned));
        parts.join("\n")
    }

    /// A ref that isn't on screen gets a reminder of what it was and where the agent saw it.
    fn explain_stale_ref(&self, error: Error) -> Error {
        match error {
            Error::ElementNotFound { target, candidates } if candidates.is_empty() => {
                match self.state.seen.get(&target) {
                    Some(seen) => Error::ElementNotFound {
                        target: format!("{target} ({seen}; not on the current screen)"),
                        candidates,
                    },
                    None => Error::ElementNotFound { target, candidates },
                }
            }
            other => other,
        }
    }

    /// Assigns session refs to a fresh snapshot.
    fn adopt(&mut self, mut snapshot: Snapshot) -> View {
        self.state.refs.assign(&mut snapshot.tree);
        let screen = snapshot
            .screen
            .activity
            .as_deref()
            .map(|a| a.split_once('/').map_or(a, |(_, activity)| activity))
            .unwrap_or("?");
        for node in snapshot.tree.iter() {
            let what = short(node);
            let what = what.strip_prefix(&node.r#ref).unwrap_or(&what).trim_start();
            self.state
                .seen
                .insert(node.r#ref.clone(), format!("{what} on {screen}"));
        }
        View {
            tree: snapshot.tree,
            screen: snapshot.screen,
        }
    }

    /// Executes the action; returns its description and its replayable form.
    async fn perform(
        &mut self,
        action: &Action,
        before: &View,
        previous: Option<&UiTree>,
    ) -> Result<(String, Action)> {
        match action {
            Action::Tap { target } => {
                let r = resolve(target, &before.tree, previous, &before.screen.obstructions)?;
                let (x, y) = r.point;
                self.control.input(&Input::Tap { x, y }).await?;
                Ok((
                    format!("tap {}", describe(target, &r)),
                    Action::Tap {
                        target: persistable(target, &r, &before.tree),
                    },
                ))
            }
            Action::LongPress {
                target,
                duration_ms,
            } => {
                let r = resolve(target, &before.tree, previous, &before.screen.obstructions)?;
                self.control
                    .input(&Input::Swipe {
                        from: r.point,
                        to: r.point,
                        duration_ms: *duration_ms,
                        hold_ms: 0,
                    })
                    .await?;
                Ok((
                    format!("long-press {}", describe(target, &r)),
                    Action::LongPress {
                        target: persistable(target, &r, &before.tree),
                        duration_ms: *duration_ms,
                    },
                ))
            }
            Action::Type {
                text,
                into,
                append,
                enter,
            } => {
                let mut view = before.clone();
                let mut recorded_into = None;
                if let Some(target) = into {
                    let r = resolve(target, &view.tree, previous, &view.screen.obstructions)?;
                    let (x, y) = r.point;
                    recorded_into = Some(persistable(target, &r, &view.tree));
                    self.control.input(&Input::Tap { x, y }).await?;
                    view = self.wait_for_focus().await?;
                }
                let field = view
                    .tree
                    .iter()
                    .find(|n| n.role == Role::Textbox && n.state.focused)
                    .ok_or_else(|| Error::ElementNotFound {
                        target: "focused text field".into(),
                        candidates: view
                            .tree
                            .iter()
                            .filter(|n| n.role == Role::Textbox)
                            .map(render_line)
                            .collect(),
                    })?;
                let secret = field.state.password;
                let value = match (append, &field.value) {
                    (true, _) if secret => {
                        return Err(Error::InvalidTarget {
                            target: field.r#ref.clone(),
                            reason: "can't append to a password field; type the whole value".into(),
                        });
                    }
                    (true, Some(current)) => format!("{current}{text}"),
                    _ => text.clone(),
                };
                let field_name = short(field);
                self.control.input(&Input::SetText { text: value }).await?;
                if *enter {
                    self.control
                        .input(&Input::Key {
                            name: "ENTER".into(),
                        })
                        .await?;
                }
                let shown = if secret {
                    "••••".to_owned()
                } else {
                    format!("{text:?}")
                };
                Ok((
                    format!("type {shown} into {field_name}"),
                    Action::Type {
                        text: if secret {
                            "<secret>".into()
                        } else {
                            text.clone()
                        },
                        into: recorded_into,
                        append: *append,
                        enter: *enter,
                    },
                ))
            }
            Action::Swipe {
                from,
                to,
                duration_ms,
            } => {
                self.control
                    .input(&Input::Swipe {
                        from: *from,
                        to: *to,
                        duration_ms: *duration_ms,
                        hold_ms: 0,
                    })
                    .await?;
                Ok((
                    format!("swipe {},{} → {},{}", from.0, from.1, to.0, to.1),
                    action.clone(),
                ))
            }
            Action::Key { name } => {
                self.control
                    .input(&Input::Key { name: name.clone() })
                    .await?;
                Ok((format!("key {name}"), action.clone()))
            }
            Action::Scroll {
                direction,
                within,
                until,
            } => {
                let (area, recorded_within) = match within {
                    Some(target) => {
                        let r =
                            resolve(target, &before.tree, previous, &before.screen.obstructions)?;
                        let bounds = r.node.map_or(before.tree.screen, |n| n.bounds);
                        (
                            inset(bounds, 10),
                            Some(persistable(target, &r, &before.tree)),
                        )
                    }
                    // Keep clear of the status bar and the gesture navigation area.
                    None => (inset(before.tree.screen, 15), None),
                };
                let gesture = scroll_gesture(*direction, area);
                let recorded = Action::Scroll {
                    direction: *direction,
                    within: recorded_within,
                    until: until.clone(),
                };
                let dir = format!("{direction:?}").to_lowercase();
                let Some(goal) = until else {
                    self.control.input(&gesture).await?;
                    return Ok((format!("scroll {dir}"), recorded));
                };
                let mut view = before.clone();
                for scrolls in 0..=MAX_SCROLLS {
                    if present(goal, &view.tree, None) {
                        return Ok((
                            format!("scroll {dir} until {goal}: found after {scrolls} scrolls"),
                            recorded,
                        ));
                    }
                    if scrolls == MAX_SCROLLS {
                        break;
                    }
                    self.control.input(&gesture).await?;
                    let settled =
                        settle(&self.control, &mut Timings::default(), Some(&view.tree)).await?;
                    let next = self.adopt(settled.snapshot);
                    if next.tree.fingerprint() == view.tree.fingerprint() {
                        break; // reached the end
                    }
                    view = next;
                }
                Err(Error::ElementNotFound {
                    target: goal.to_string(),
                    candidates: Vec::new(),
                })
            }
        }
    }

    async fn wait_for_focus(&mut self) -> Result<View> {
        let deadline = Instant::now() + FOCUS_TIMEOUT;
        loop {
            tokio::time::sleep(Duration::from_millis(80)).await;
            let snapshot = self.control.snapshot().await?;
            let view = self.adopt(snapshot);
            let focused = view
                .tree
                .iter()
                .any(|n| n.role == Role::Textbox && n.state.focused);
            if focused || Instant::now() >= deadline {
                return Ok(view);
            }
        }
    }
}

/// Diff against `previous` unless the screen changed wholesale; returns (diff, new screen, body).
fn report(previous: Option<&View>, now: &View) -> (Option<TreeDiff>, bool, String) {
    let Some(previous) = previous else {
        return (None, true, render(&now.tree));
    };
    let d = diff(&previous.tree, &now.tree);
    let churn = d.added.len() + d.removed.len();
    let total = previous.tree.iter().count() + now.tree.iter().count();
    let new_screen = previous.screen.activity != now.screen.activity || churn * 10 > total * 6;
    if new_screen {
        return (None, true, render(&now.tree));
    }
    let mut body = if d.is_empty() {
        "no visible change".to_owned()
    } else {
        render_diff(&d)
    };
    let opaque = render_opaque(&now.tree);
    if !opaque.is_empty() {
        body = format!("{body}\n{opaque}");
    }
    (Some(d), false, body)
}

/// The session's app and the distinct packages of the screens' activities.
fn packages_of(app: Option<&str>, screens: &[&ScreenInfo]) -> Vec<String> {
    let mut packages: Vec<String> = Vec::new();
    for package in app.map(str::to_owned).into_iter().chain(
        screens
            .iter()
            .filter_map(|s| s.activity.as_deref()?.split_once('/'))
            .map(|(package, _)| package.to_owned()),
    ) {
        if !packages.contains(&package) {
            packages.push(package);
        }
    }
    packages
}

fn present(target: &Target, tree: &UiTree, previous: Option<&UiTree>) -> bool {
    !matches!(
        resolve(target, tree, previous, &[]),
        Err(Error::ElementNotFound { .. })
    )
}

/// Refs only mean something within a session; recordings store a selector instead.
fn persistable(target: &Target, resolved: &Resolved, tree: &UiTree) -> Target {
    match (target, resolved.node) {
        (Target::Ref(_), Some(node)) => Target::Selector(selector_for(node, tree)),
        _ => target.clone(),
    }
}

fn describe(target: &Target, resolved: &Resolved) -> String {
    match resolved.node {
        Some(node) => short(node),
        None => target.to_string(),
    }
}

/// `e5 item "Network & internet"`.
fn short(node: &UiNode) -> String {
    match &node.label {
        Some(label) => format!("{} {} {label:?}", node.r#ref, node.role.as_str()),
        None => format!("{} {}", node.r#ref, node.role.as_str()),
    }
}

fn inset(r: Rect, percent: i32) -> Rect {
    let dx = r.width() * percent / 100;
    let dy = r.height() * percent / 100;
    Rect::new(r.left + dx, r.top + dy, r.right - dx, r.bottom - dy)
}

/// A swipe across `area` that reveals content in `direction`: to see what is below, the finger
/// moves up.
fn scroll_gesture(direction: Direction, area: Rect) -> Input {
    let (cx, cy) = area.center();
    let (from, to) = match direction {
        Direction::Down => ((cx, area.bottom), (cx, area.top)),
        Direction::Up => ((cx, area.top), (cx, area.bottom)),
        Direction::Right => ((area.right, cy), (area.left, cy)),
        Direction::Left => ((area.left, cy), (area.right, cy)),
    };
    Input::Swipe {
        from,
        to,
        duration_ms: SCROLL_MS,
        hold_ms: SCROLL_HOLD_MS,
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}
