//! MCP server: the session engine as tools for agents (functional design §4.2).
//!
//! One session per connection, kept in memory; the device is connected on first use. Results are
//! the same compact text the CLI prints. `structuredContent` is not sent: clients commonly forward
//! it to the model next to the text, which would double the tokens of every observation.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use mdh_control::{
    ActOutcome, Action, ConnectOptions, Control, DEFAULT_DEVICE_FILE, Direction, Observation,
    RunOptions, Session, Target, launch_text,
};
use mdh_core::output::Timings;
use mdh_core::{Error, LogLevel, Result};
use mdh_observe::{CrashKind, LogDigest};
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, Meta, ProgressNotificationParam,
    ServerCapabilities, ServerInfo,
};
use rmcp::service::{Peer, RoleServer};
use rmcp::transport::stdio;
use rmcp::{ErrorData, ServerHandler, ServiceExt, schemars, tool, tool_handler, tool_router};
use serde::Deserialize;
use tokio::sync::Mutex;

const INSTRUCTIONS: &str = "\
mobile-dev-harness drives an Android app for you. Start with mdh_observe: it shows the screen as a \
compact tree where every element has a ref like e12 that stays the same for the whole session. \
Targets are a ref, coordinates (\"100,200\"), a selector (\"id=login\", \"text=Sign in\", \
\"text~=sign\", \"role=switch\", combined with ';') or a plain label. Every action waits for the UI \
to settle and returns only what changed (+ added, ~ changed, - removed), or the whole tree when the \
screen changed; don't observe again after an action. Lines starting with !! are crashes of the app: \
fix them before anything else. Prefer refs and labels over coordinates. After changing code, call \
mdh_impact to learn which screens the change reaches, then verify each of them with mdh_verify, not \
only the one you edited; a change is done when the verdict passes. Save what you did as a flow \
(mdh_flow) so the check can be repeated.";

const SCREENSHOT_EDGE: u32 = 1024;
/// Saved flows, relative to the server's working directory.
const FLOWS_DIR: &str = ".mdh/flows";

/// One MCP connection: a lazily connected session.
#[derive(Clone)]
pub struct MdhServer {
    session: Arc<Mutex<Option<Session>>>,
    device: Arc<Mutex<Option<String>>>,
    #[expect(dead_code, reason = "read by the tool_handler macro")]
    tool_router: ToolRouter<Self>,
}

/// Serves MCP on stdin/stdout until the client disconnects.
pub async fn serve_stdio(
    device: Option<String>,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let server = MdhServer::new(device);
    let service = server.clone().serve(stdio()).await?;
    service.waiting().await?;
    // Leave the device as it was found.
    if let Some(session) = server.session.lock().await.as_mut()
        && session.animations_off()
    {
        let _ = session.animations(true).await;
    }
    Ok(())
}

impl MdhServer {
    pub fn new(device: Option<String>) -> Self {
        Self {
            session: Arc::default(),
            device: Arc::new(Mutex::new(device)),
            tool_router: Self::tool_router(),
        }
    }

    /// A server whose session already uses `control`; for tests with scripted drivers.
    pub fn with_control(control: Control) -> Self {
        let server = Self::new(None);
        *server.session.try_lock().expect("fresh mutex") = Some(Session::open(control, None));
        server
    }

    /// Runs `f` on the session, connecting to the device first if needed.
    async fn with_session<T>(&self, f: impl AsyncFnOnce(&mut Session) -> Result<T>) -> Result<T> {
        let mut guard = self.session.lock().await;
        if guard.is_none() {
            let device = self.device.lock().await.clone();
            // No one to ask here: the agent gets the options in the error and asks its user.
            let connected = Control::connect_with(ConnectOptions {
                requested: device.as_deref(),
                default_file: Path::new(DEFAULT_DEVICE_FILE),
                ask: None,
                headless: false,
            })
            .await?;
            let mut session = Session::open(connected.control, None);
            let _ = connected.started; // MCP sessions live in memory: always fresh on connect.
            if let Some(notice) = connected.notice {
                session.notify(notice);
            }
            *guard = Some(session);
        }
        f(guard.as_mut().expect("connected above")).await
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct StatusParams {
    /// Switch to this device (adb serial, or a running emulator's AVD name) and make it the
    /// project's default. Starts a new session.
    pub device: Option<String>,
    /// Start this emulator (AVD name; empty for the project's or the newest one) and switch to it.
    /// Only after the user agreed: it takes up to a few minutes and uses memory.
    pub start_emulator: Option<String>,
    /// Forget refs and recorded steps and stop the on-device helper (frees UiAutomation for other tools).
    pub reset: Option<bool>,
    /// false turns system animations off (screens settle sooner); true restores them. Restored
    /// when the session resets or the connection closes.
    pub animations: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ObserveParams {
    /// Only what changed since the last observation or action.
    pub diff: Option<bool>,
    /// Also return a screenshot (JPEG, long edge 1024 px). Use when the tree reports opaque regions
    /// or visual details matter.
    pub screenshot: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ActParams {
    /// Actions to run in order. Stops at the first failure; each reports what changed.
    pub actions: Vec<ActSpec>,
}

/// One action. Targets: a ref (`e12`), coordinates (`100,200`), a selector (`id=…`, `text=…`,
/// `text~=…`, `role=…`, `index=…`, combined with `;`) or a label.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ActSpec {
    Tap {
        target: String,
    },
    LongPress {
        target: String,
        /// Default 800.
        duration_ms: Option<u32>,
    },
    /// Sets the focused field's text (any Unicode), after tapping `into` if given.
    Type {
        text: String,
        into: Option<String>,
        /// Append to the current text instead of replacing it.
        append: Option<bool>,
        /// Press ENTER afterwards.
        enter: Option<bool>,
    },
    Swipe {
        from: [i32; 2],
        to: [i32; 2],
        /// Default 300.
        duration_ms: Option<u32>,
    },
    /// Reveals content in `direction` (`down` shows what is below), optionally until `until` is on screen.
    Scroll {
        direction: ScrollDirection,
        within: Option<String>,
        until: Option<String>,
    },
    /// Android key name without `KEYCODE_`, e.g. BACK, HOME, ENTER.
    Key {
        name: String,
    },
}

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ScrollDirection {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RunParams {
    /// A directory inside the Gradle project; default: the server's working directory.
    pub project: Option<String>,
    /// Application module such as `app`, when the build has several.
    pub module: Option<String>,
    /// Build variant such as `debug` or `freeDebug`; default: the debug variant.
    pub variant: Option<String>,
    /// Build first (default true); false reuses the last built APK.
    pub build: Option<bool>,
    /// Grant all runtime permissions on install.
    pub grant: Option<bool>,
    /// Uninstall first: replaces an app signed with another key or a newer version; clears its data.
    pub reinstall: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct VerifyParams {
    /// Checks on the app as it is now: `visible "Sign in"`, `not visible id=error`,
    /// `enabled|disabled|checked|unchecked|focused TARGET`, `text TARGET == "value"`,
    /// `text TARGET ~= "part"`, `screen .LoginActivity`, `log ~= "text"`, `no log ~= "text"`.
    /// `no crash` is always checked.
    pub checks: Option<Vec<String>>,
    /// Saved flows to replay instead (names in .mdh/flows); each gets its own verdict.
    pub flows: Option<Vec<String>>,
    /// Replay the saved flows that pass the screens the uncommitted change reaches (as found by
    /// mdh_impact).
    pub changed: Option<bool>,
    /// Seconds checks may take to start holding, default 3.
    pub timeout_s: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct VisualParams {
    pub command: VisualCommand,
    /// `check`: compare the screen with (or record) the structural baseline kept under this name.
    /// `approve`: the name or flow whose candidate baselines become the baselines (default: all).
    pub baseline: Option<String>,
    /// `check`: rules to apply — `touch_target`, `label`, `overlap`, `obscured`, `duplicate_label`;
    /// default all.
    pub rules: Option<Vec<String>>,
    /// `check`: elements to leave out of the baseline comparison (dynamic content), as targets.
    pub ignore: Option<Vec<String>>,
    /// `check`: also check the screen at a larger font (`font_scale`), in dark mode (`dark`) and
    /// right to left (`rtl`); each is switched on, checked against the default and restored.
    pub configs: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum VisualCommand {
    Check,
    Approve,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PerfParams {
    pub command: PerfCommand,
    /// `startup`: package or package/activity (default: the session's app). `explain`: the app's
    /// package.
    pub app: Option<String>,
    /// `flow`: the saved flow to measure.
    pub flow: Option<String>,
    /// `startup`: measure the hot start too.
    pub hot: Option<bool>,
    /// `startup`, `flow`: measured runs, default 5 (or the flow's `perf: runs`).
    pub runs: Option<usize>,
    /// `startup`, `flow`: capture and explain a Perfetto trace even if nothing regressed.
    pub trace: Option<bool>,
    /// `approve`: the scope whose latest measurements become the baselines: a flow's name or
    /// `startup-<package>` (default: all).
    pub scope: Option<String>,
    /// `setup`: the user agreed to download Perfetto's trace processor. Never set it without
    /// asking them.
    pub consent: Option<bool>,
    /// `explain`: the trace file a verdict named.
    pub path: Option<String>,
    /// `explain`: the trace is of a cold start (default: of a flow).
    pub startup: Option<bool>,
}

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PerfCommand {
    Startup,
    Flow,
    Approve,
    Setup,
    Explain,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FlowParams {
    pub command: FlowCommand,
    /// `save`, `show`: the flow's name (letters, digits, `-`, `_`).
    pub name: Option<String>,
    /// `save`: only the last N recorded steps (default: all of the session's steps).
    pub last: Option<usize>,
    /// `save`: checks to run after the last step, same syntax as mdh_verify.
    pub checks: Option<Vec<String>>,
    /// `save`: overwrite an existing flow.
    pub force: Option<bool>,
}

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FlowCommand {
    Save,
    List,
    Show,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ImpactParams {
    /// A directory inside the project; default: the server's working directory.
    pub project: Option<String>,
    /// Revision to compare the working tree with, such as `main` or `HEAD~1`; default `HEAD`
    /// (the uncommitted change).
    pub base: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WaitParams {
    pub target: String,
    /// Wait for the target to disappear instead.
    pub gone: Option<bool>,
    /// Seconds, default 10.
    pub timeout_s: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct LogsParams {
    /// Minimum level, default warn.
    pub level: Option<Level>,
    /// Most recent lines, default 50.
    pub lines: Option<usize>,
}

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Verbose,
    Debug,
    Info,
    Warn,
    Error,
}

// MCP requires an object at the root of every input schema, so this is a struct rather than a
// tagged enum (which would generate a root `oneOf`).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AppParams {
    pub command: AppCommand,
    /// `launch`: a package (its launcher activity) or `package/activity`; it becomes the session's
    /// app, whose logs and crashes are always watched. `stop`, `clear_data`, `grant`, `revoke`: a
    /// package (default: the session's app). `install`: an APK path on the host. `open`: a deep
    /// link such as `myapp://settings`.
    pub app: Option<String>,
    /// `install` only: grant all runtime permissions.
    pub grant: Option<bool>,
    /// `grant`, `revoke`: the permission, e.g. `CAMERA` or `android.permission.POST_NOTIFICATIONS`.
    pub permission: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AppCommand {
    Launch,
    Stop,
    Install,
    Open,
    ClearData,
    Grant,
    Revoke,
}

#[tool_router]
impl MdhServer {
    #[tool(
        description = "Device and session status: which device, the last screen, recorded steps. Switches devices (remembered for the project), starts an emulator once the user agreed, resets the session, turns animations off. When no device or several are online, other tools fail with the options; ask the user, then call this."
    )]
    async fn mdh_status(
        &self,
        Parameters(p): Parameters<StatusParams>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        let default_file = Path::new(DEFAULT_DEVICE_FILE);
        let mut notes = Vec::new();
        if let Some(avd) = p.start_emulator {
            let avd = Some(avd).filter(|a| !a.trim().is_empty());
            match mdh_control::start_emulator(avd.as_deref(), false, default_file).await {
                Ok((device, text)) => {
                    notes.push(text);
                    *self.device.lock().await = Some(device.id);
                    *self.session.lock().await = None;
                }
                Err(e) => return Ok(error_result(&e)),
            }
        }
        if let Some(device) = p.device {
            match mdh_control::use_device(&device, default_file).await {
                Ok(text) => notes.push(text),
                Err(e) => return Ok(error_result(&e)),
            }
            *self.device.lock().await = Some(device);
            *self.session.lock().await = None;
        }
        let reset = p.reset.unwrap_or(false);
        let animations = p.animations;
        let result = self
            .with_session(async |s| {
                if reset {
                    s.reset().await?;
                }
                let mut text = s.summary().text();
                if let Some(on) = animations {
                    text = format!("{}\n{text}", s.animations(on).await?);
                }
                if !notes.is_empty() {
                    text = format!("{}\n{text}", notes.join("\n"));
                }
                Ok(text)
            })
            .await;
        Ok(text_result(result))
    }

    #[tool(
        description = "Observe the current screen: activity, compact UI tree with refs, new log warnings/errors and crashes. Optionally only the diff since the last look, or a screenshot."
    )]
    async fn mdh_observe(
        &self,
        Parameters(p): Parameters<ObserveParams>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        let diff = p.diff.unwrap_or(false);
        let screenshot = p.screenshot.unwrap_or(false);
        let result = self
            .with_session(async |s| {
                let observation = s.observe(diff, &mut Timings::default()).await?;
                let image = if screenshot {
                    Some(
                        s.control()
                            .capture(SCREENSHOT_EDGE, &mut Timings::default())
                            .await?,
                    )
                } else {
                    None
                };
                Ok((observation, image))
            })
            .await;
        Ok(match result {
            Ok((observation, image)) => {
                let mut result = observed(&observation.text, observation.logs.as_ref());
                if let Some(image) = image {
                    result.content.push(ContentBlock::image(
                        base64::engine::general_purpose::STANDARD.encode(&image.bytes),
                        "image/jpeg",
                    ));
                }
                result
            }
            Err(e) => error_result(&e),
        })
    }

    #[tool(
        description = "Perform one or more actions (tap, long_press, type, swipe, scroll, key). Each waits for the UI to settle and reports what changed since you last looked, plus new log errors and crashes."
    )]
    async fn mdh_act(
        &self,
        Parameters(p): Parameters<ActParams>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        if p.actions.is_empty() {
            return Err(ErrorData::invalid_params("actions must not be empty", None));
        }
        let mut texts = Vec::new();
        let mut crash = None;
        let mut failure = None;
        for spec in p.actions {
            let outcome: Result<ActOutcome> = match to_action(spec) {
                Ok(action) => {
                    self.with_session(async |s| s.act(action, &mut Timings::default()).await)
                        .await
                }
                Err(e) => Err(e),
            };
            match outcome {
                Ok(outcome) => {
                    texts.push(outcome.text.clone());
                    crash = crash_error(outcome.logs.as_ref());
                    if crash.is_some() {
                        break;
                    }
                }
                Err(e) => {
                    failure = Some(e);
                    break;
                }
            }
        }
        if let Some(e) = failure.as_ref().or(crash.as_ref()) {
            texts.push(error_text(e));
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                texts.join("\n\n"),
            )]));
        }
        Ok(CallToolResult::success(vec![ContentBlock::text(
            texts.join("\n\n"),
        )]))
    }

    #[tool(
        description = "Build the app with Gradle, install it if it changed, restart it and report its first settled screen. Build failures come back as file:line diagnostics with the source line. Reports progress while building."
    )]
    async fn mdh_run(
        &self,
        Parameters(p): Parameters<RunParams>,
        meta: Meta,
        client: Peer<RoleServer>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        let options = RunOptions {
            project: p.project.map_or_else(|| ".".into(), Into::into),
            module: p.module,
            variant: p.variant,
            build: p.build.unwrap_or(true),
            grant: p.grant.unwrap_or(false),
            reinstall: p.reinstall.unwrap_or(false),
        };
        // Gradle task lines become progress notifications, at most two per second.
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let progress = meta.get_progress_token().map(|token| {
            tokio::spawn(async move {
                let mut count = 0u32;
                let mut last: Option<std::time::Instant> = None;
                while let Some(task) = rx.recv().await {
                    count += 1;
                    if last.is_none_or(|l| l.elapsed() >= Duration::from_millis(500)) {
                        last = Some(std::time::Instant::now());
                        let note = ProgressNotificationParam::new(token.clone(), f64::from(count))
                            .with_message(format!("building {task}"));
                        let _ = client.notify_progress(note).await;
                    }
                }
            })
        });
        let result = self
            .with_session(async |s| {
                s.run(options, &mut Timings::default(), |task| {
                    let _ = tx.send(task.to_owned());
                })
                .await
            })
            .await;
        drop(tx);
        if let Some(progress) = progress {
            let _ = progress.await;
        }
        Ok(match result {
            Ok(mut report) => match report.take_failure() {
                Some(e) => CallToolResult::error(vec![ContentBlock::text(format!(
                    "{}\n\n{}",
                    report.text,
                    error_text(&e)
                ))]),
                None => observed(
                    &report.text,
                    report.observation.as_ref().and_then(|o| o.logs.as_ref()),
                ),
            },
            Err(e) => error_result(&e),
        })
    }

    #[tool(
        description = "Wait until a target is on screen (or gone), then report the screen. Fails after the timeout."
    )]
    async fn mdh_wait(
        &self,
        Parameters(p): Parameters<WaitParams>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        let gone = p.gone.unwrap_or(false);
        let timeout = Duration::from_secs(p.timeout_s.unwrap_or(10));
        let result: Result<Observation> = match Target::parse(&p.target) {
            Ok(target) => {
                self.with_session(async |s| {
                    s.wait(&target, gone, timeout, &mut Timings::default())
                        .await
                })
                .await
            }
            Err(e) => Err(e),
        };
        Ok(match result {
            Ok(o) => observed(&o.text, o.logs.as_ref()),
            Err(e) => error_result(&e),
        })
    }

    #[tool(
        description = "Recent log lines (default warn and above) and crash reports of the session's app and the app in front, over the last 10 minutes."
    )]
    async fn mdh_logs(
        &self,
        Parameters(p): Parameters<LogsParams>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        let level = match p.level.unwrap_or(Level::Warn) {
            Level::Verbose => LogLevel::Verbose,
            Level::Debug => LogLevel::Debug,
            Level::Info => LogLevel::Info,
            Level::Warn => LogLevel::Warn,
            Level::Error => LogLevel::Error,
        };
        let lines = p.lines.unwrap_or(50);
        let result = self
            .with_session(async |s| Ok(s.logs(level, lines).await?.text()))
            .await;
        Ok(text_result(result))
    }

    #[tool(
        description = "What a code change reaches, from static analysis (no device, no build): changed declarations, what they call before vs. after, the screens affected and how to reach them, call sites of changed signatures, and what to verify (functional, UI, performance, compatibility, tests). Run it after editing and before verifying, so every affected screen gets checked."
    )]
    async fn mdh_impact(
        &self,
        Parameters(p): Parameters<ImpactParams>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        let options = mdh_impact::Options {
            project: p.project.map_or_else(|| ".".into(), Into::into),
            base: p.base.unwrap_or_else(|| "HEAD".into()),
        };
        let result = tokio::task::spawn_blocking(move || {
            let mut report = mdh_impact::analyze(&options)?;
            report.verify.flows =
                mdh_verify::flows_for(&report, &mdh_verify::FlowStore::new(FLOWS_DIR))?;
            Ok(report)
        })
        .await
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(text_result(result.map(|r| mdh_impact::render(&r))))
    }

    #[tool(
        description = "Verify the app and get a verdict: checks on the current screen and logs, or saved flows replayed from a fresh start. Each check shows what was observed when it fails; evidence (screenshot, tree, logs) is saved to .mdh/runs. A change is done when its verdict passes."
    )]
    async fn mdh_verify(
        &self,
        Parameters(p): Parameters<VerifyParams>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        let timeout = Duration::from_secs(p.timeout_s.unwrap_or(3));
        let verify = mdh_verify::VerifyOptions {
            timeout,
            checks: check_kinds(),
            ..mdh_verify::VerifyOptions::default()
        };
        let result = match (p.flows.filter(|f| !f.is_empty()), p.checks) {
            _ if p.changed == Some(true) => {
                let options = mdh_verify::FlowOptions {
                    verify,
                    ..mdh_verify::FlowOptions::default()
                };
                self.with_session(async |s| {
                    let store = mdh_verify::FlowStore::new(FLOWS_DIR);
                    let mut timings = Timings::default();
                    let project = std::path::Path::new(".");
                    Ok(
                        mdh_verify::run_changed(s, &store, project, "HEAD", &options, &mut timings)
                            .await?
                            .text,
                    )
                })
                .await
            }
            (Some(names), _) => {
                let options = mdh_verify::FlowOptions {
                    verify,
                    ..mdh_verify::FlowOptions::default()
                };
                self.with_session(async |s| {
                    let store = mdh_verify::FlowStore::new(FLOWS_DIR);
                    let mut timings = Timings::default();
                    Ok(
                        mdh_verify::run_flows(s, &store, &names, &options, &mut timings)
                            .await?
                            .text,
                    )
                })
                .await
            }
            (None, checks) => {
                let checks: Result<Vec<mdh_verify::Assertion>> = checks
                    .unwrap_or_default()
                    .iter()
                    .map(|c| mdh_verify::Assertion::parse(c))
                    .collect();
                match checks {
                    Ok(checks) => {
                        self.with_session(async |s| {
                            let mut timings = Timings::default();
                            Ok(mdh_verify::verify(s, checks, &verify, &mut timings)
                                .await?
                                .text)
                        })
                        .await
                    }
                    Err(e) => Err(e),
                }
            }
        };
        Ok(text_result(result))
    }

    #[tool(
        description = "UI consistency of the current screen: rule checks (touch targets ≥ 48 dp, labels, overlapping controls, controls under the system bars, duplicate labels) and, with `baseline`, a structural comparison with the stored baseline that reports elements added, missing, moved, resized or with changed text. `approve` accepts the deviations as the new baseline. Flows run these checks too (configured by the flow's `visual:` section)."
    )]
    async fn mdh_visual(
        &self,
        Parameters(p): Parameters<VisualParams>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        if let VisualCommand::Approve = p.command {
            return Ok(text_result(mdh_visual::approve(p.baseline.as_deref())));
        }
        let rules: serde_json::Value = match p.rules {
            Some(list) => list.into(),
            None => "all".into(),
        };
        let options = mdh_verify::VerifyOptions {
            checks: check_kinds(),
            config: Some(serde_json::json!({
                "rules": rules,
                "baseline": p.baseline.is_some(),
                "ignore": p.ignore.unwrap_or_default(),
                "configs": p.configs.unwrap_or_default(),
            })),
            scope: p.baseline,
            ..mdh_verify::VerifyOptions::default()
        };
        let result = self
            .with_session(async |s| {
                let mut timings = Timings::default();
                Ok(mdh_verify::verify(s, Vec::new(), &options, &mut timings)
                    .await?
                    .text)
            })
            .await;
        Ok(text_result(result))
    }

    #[tool(
        description = "Performance against baselines and budgets, over repeated runs (median, noise and run count reported; a change must exceed the noise to count). `startup`: cold (and hot) start time of the app. `flow`: frames (janky share, p90, p99), memory and CPU while a saved flow runs; budgets come from the flow's `perf:` section. The first measurement records the baseline; later ones fail on a regression, and a Perfetto trace then says what the main thread did (what's slow, by name). `approve` accepts new measurements as baselines. Analyzing traces needs Perfetto's trace processor: if a verdict says it's missing, ask the user, and only if they agree call `setup` with `consent: true` (a 14 MB download); `explain` then summarizes a kept trace. Takes 10–60 s."
    )]
    async fn mdh_perf(
        &self,
        Parameters(p): Parameters<PerfParams>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        let missing = |what: &str| Error::InvalidFlow {
            flow: String::new(),
            reason: format!("pass `{what}`"),
        };
        let options = mdh_perf::PerfOptions {
            runs: p.runs.unwrap_or(5).max(1),
            trace: p.trace.unwrap_or(false),
            ..mdh_perf::PerfOptions::default()
        };
        let result = match p.command {
            PerfCommand::Approve => mdh_perf::approve(p.scope.as_deref()),
            PerfCommand::Setup => mdh_perf::setup(p.consent.unwrap_or(false)),
            PerfCommand::Explain => match (p.path, p.app) {
                (Some(path), Some(app)) => {
                    let scenario = if p.startup == Some(true) {
                        mdh_perf::Scenario::Startup
                    } else {
                        mdh_perf::Scenario::Flow
                    };
                    mdh_perf::explain(Path::new(&path), &app, scenario).map(|l| l.join("\n"))
                }
                (None, _) => Err(missing("path")),
                (_, None) => Err(missing("app")),
            },
            PerfCommand::Startup => {
                let hot = p.hot.unwrap_or(false);
                self.with_session(async |s| {
                    let app = s.package_or_app(p.app.as_deref())?;
                    Ok(mdh_perf::startup(s, &app, hot, &options).await?.text)
                })
                .await
            }
            PerfCommand::Flow => match p.flow {
                Some(name) => {
                    let runs = p.runs;
                    self.with_session(async |s| {
                        let mut flow = mdh_verify::FlowStore::new(FLOWS_DIR).load(&name)?;
                        if let Some(runs) = runs
                            && let Some(perf) = flow.perf.as_mut().and_then(|v| v.as_object_mut())
                        {
                            perf.insert("runs".into(), runs.into());
                        }
                        let mut timings = Timings::default();
                        Ok(mdh_perf::flow(s, &flow, &options, &mut timings).await?.text)
                    })
                    .await
                }
                None => Err(missing("flow")),
            },
        };
        Ok(text_result(result))
    }

    #[tool(
        description = "Flows are replayable tests made from what you did in this session: save the recorded steps (with checks to run at the end), list saved flows, or show one's YAML (in .mdh/flows; edit the file to change it). Replay them with mdh_verify."
    )]
    async fn mdh_flow(
        &self,
        Parameters(p): Parameters<FlowParams>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        let store = mdh_verify::FlowStore::new(FLOWS_DIR);
        let name = || {
            p.name.clone().ok_or_else(|| Error::InvalidFlow {
                flow: String::new(),
                reason: "pass `name`".into(),
            })
        };
        let result = match p.command {
            FlowCommand::List => store.list(),
            FlowCommand::Show => name().and_then(|n| store.load(&n)).map(|f| f.to_yaml()),
            FlowCommand::Save => match name() {
                Ok(n) => {
                    let checks = p.checks.clone().unwrap_or_default();
                    let (last, force) = (p.last, p.force.unwrap_or(false));
                    self.with_session(async |s| {
                        mdh_verify::save_flow(s, &store, &n, last, &checks, force)
                            .map(|saved| saved.text)
                    })
                    .await
                }
                Err(e) => Err(e),
            },
        };
        Ok(text_result(result))
    }

    #[tool(
        description = "App lifecycle and state: launch, stop, install, open a deep link, clear its data, grant or revoke a permission."
    )]
    async fn mdh_app(
        &self,
        Parameters(p): Parameters<AppParams>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        let needs = |what: &str| Error::InvalidTarget {
            target: String::new(),
            reason: format!("pass `{what}`"),
        };
        let result = self
            .with_session(async |s| {
                let app = p.app.as_deref();
                match p.command {
                    AppCommand::Launch => Ok(launch_text(
                        &s.launch(app.ok_or_else(|| needs("app"))?).await?,
                    )),
                    AppCommand::Stop => {
                        let package = s.package_or_app(app)?;
                        s.control().stop(&package).await?;
                        Ok(format!("stopped {package}"))
                    }
                    AppCommand::Install => {
                        let apk = app.ok_or_else(|| needs("app"))?;
                        s.control()
                            .install(std::path::Path::new(apk), p.grant.unwrap_or(false))
                            .await?;
                        Ok(format!("installed {apk}"))
                    }
                    AppCommand::Open => {
                        let uri = app.ok_or_else(|| needs("app"))?;
                        let package = s.app().map(str::to_owned);
                        Ok(launch_text(&s.open_uri(uri, package.as_deref()).await?))
                    }
                    AppCommand::ClearData => s.clear_data(app).await,
                    AppCommand::Grant | AppCommand::Revoke => {
                        let permission =
                            p.permission.as_deref().ok_or_else(|| needs("permission"))?;
                        s.set_permission(app, permission, matches!(p.command, AppCommand::Grant))
                            .await
                    }
                }
            })
            .await;
        Ok(text_result(result))
    }
}

#[tool_handler]
impl ServerHandler for MdhServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "mobile-dev-harness",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(INSTRUCTIONS)
    }
}

fn to_action(spec: ActSpec) -> Result<Action> {
    let target = |s: &str| Target::parse(s);
    let optional = |s: Option<String>| s.as_deref().map(target).transpose();
    Ok(match spec {
        ActSpec::Tap { target: t } => Action::Tap {
            target: target(&t)?,
        },
        ActSpec::LongPress {
            target: t,
            duration_ms,
        } => Action::LongPress {
            target: target(&t)?,
            duration_ms: duration_ms.unwrap_or(800),
        },
        ActSpec::Type {
            text,
            into,
            append,
            enter,
        } => Action::Type {
            text,
            into: optional(into)?,
            append: append.unwrap_or(false),
            enter: enter.unwrap_or(false),
        },
        ActSpec::Swipe {
            from,
            to,
            duration_ms,
        } => Action::Swipe {
            from: (from[0], from[1]),
            to: (to[0], to[1]),
            duration_ms: duration_ms.unwrap_or(300),
        },
        ActSpec::Scroll {
            direction,
            within,
            until,
        } => Action::Scroll {
            direction: match direction {
                ScrollDirection::Up => Direction::Up,
                ScrollDirection::Down => Direction::Down,
                ScrollDirection::Left => Direction::Left,
                ScrollDirection::Right => Direction::Right,
            },
            within: optional(within)?,
            until: optional(until)?,
        },
        ActSpec::Key { name } => Action::Key {
            name: name.to_uppercase(),
        },
    })
}

/// An observation's text; a crash of the app marks the result as an error (like exit code 5).
fn observed(text: &str, logs: Option<&LogDigest>) -> CallToolResult {
    match crash_error(logs) {
        Some(e) => CallToolResult::error(vec![ContentBlock::text(format!(
            "{text}\n\n{}",
            error_text(&e)
        ))]),
        None => CallToolResult::success(vec![ContentBlock::text(text)]),
    }
}

fn crash_error(logs: Option<&LogDigest>) -> Option<Error> {
    logs?
        .crashes
        .iter()
        .find(|c| c.of_app && c.kind != CrashKind::Died)
        .map(|c| Error::AppCrashed {
            package: c.package.clone().unwrap_or_else(|| "the app".into()),
            summary: c.summary.clone(),
        })
}

fn text_result(result: Result<String>) -> CallToolResult {
    match result {
        Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
        Err(e) => error_result(&e),
    }
}

fn error_result(e: &Error) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(error_text(e))])
}

/// `error[ELEMENT_NOT_FOUND]: …` and a hint; the stable code lets agents branch on it.
fn error_text(e: &Error) -> String {
    format!("error[{}]: {e}\nhint: {}", e.code().as_str(), e.hint())
}

/// Check kinds that run next to the functional checks in every verification.
fn check_kinds() -> Vec<std::sync::Arc<dyn mdh_verify::Check>> {
    vec![std::sync::Arc::new(mdh_visual::Visual::default())]
}
