mod doctor;
mod output;
mod render;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand, ValueEnum};
use mdh_control::{
    ActOutcome, Action, Ask, ConnectOptions, Control, DEFAULT_DEVICE_FILE, Direction, Inventory,
    Observation, RunOptions, RunReport, Session, Target,
};
use mdh_core::output::Timings;
use mdh_core::{Avd, Device, DeviceState, Error, LogLevel, Result};
use mdh_observe::{CrashKind, LogDigest};

use crate::output::{Human, finish};
use crate::render::Done;

/// Where CLI invocations keep session state between calls, relative to the working directory.
const SESSION_FILE: &str = ".mdh/session.json";
/// Saved flows, relative to the working directory (committed with the project).
const FLOWS_DIR: &str = ".mdh/flows";

#[derive(Parser)]
#[command(name = "mdh", version, about)]
struct Cli {
    /// Emit machine-readable JSON instead of human-readable text
    #[arg(long, global = true)]
    json: bool,

    /// Device to use (adb serial); defaults to the only online device
    #[arg(long, global = true)]
    device: Option<String>,

    #[command(subcommand)]
    command: Command,
}

/// TARGET is a ref (`e12`), coordinates (`100,200`), a selector (`id=…`, `text=…`, `text~=…`,
/// `role=…`, `index=…`, combined with `;`) or a label.
#[derive(Subcommand)]
enum Command {
    /// Check that the local toolchain (SDK, adb, emulator, JDK) is ready
    Doctor,
    /// Set the project up for mdh: .mdh/ (flows committed, state ignored) and a "how to verify"
    /// section in AGENTS.md for coding agents
    Init {
        /// Directory inside the project (default: the current one)
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Leave AGENTS.md alone
        #[arg(long)]
        no_agents_md: bool,
    },
    /// Claude Code hooks (used by the plugin): read the hook's JSON on stdin, answer on stdout
    #[command(hide = true)]
    Hook { event: HookEvent },
    /// List connected devices and the emulators that can be started; `use` sets the default
    Devices {
        #[command(subcommand)]
        command: Option<DevicesCommand>,
    },
    /// Start or stop emulators
    Emulator {
        #[command(subcommand)]
        command: EmulatorCommand,
    },
    /// Show the current screen as a compact UI tree
    Observe {
        /// Only what changed since the last observation or action
        #[arg(long)]
        diff: bool,
    },
    /// Save a downscaled JPEG screenshot
    Screenshot {
        #[arg(short, long, default_value = "screenshot.jpg")]
        output: PathBuf,
        /// Longest edge in pixels
        #[arg(long, default_value_t = 1024)]
        max_edge: u32,
    },
    /// Tap an element; reports what changed
    Tap { target: String },
    /// Long-press an element; reports what changed
    LongPress {
        target: String,
        #[arg(long, default_value_t = 800)]
        duration_ms: u32,
    },
    /// Set the focused field's text (any Unicode); reports what changed
    Type {
        text: String,
        /// Tap this field first
        #[arg(long)]
        into: Option<String>,
        /// Append to the current text instead of replacing it
        #[arg(long)]
        append: bool,
        /// Press ENTER afterwards
        #[arg(long)]
        enter: bool,
    },
    /// Swipe between device coordinates; reports what changed
    Swipe {
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        #[arg(long, default_value_t = 300)]
        duration_ms: u32,
    },
    /// Scroll to reveal content in a direction; reports what changed
    Scroll {
        direction: Dir,
        /// Scroll inside this element instead of the screen
        #[arg(long = "in")]
        within: Option<String>,
        /// Keep scrolling (up to 10 times) until this target is on screen
        #[arg(long)]
        until: Option<String>,
    },
    /// Press a key, e.g. BACK, HOME, ENTER; reports what changed
    Key { name: String },
    /// Wait until a target appears (or disappears with --gone)
    Wait {
        target: String,
        #[arg(long)]
        gone: bool,
        /// Seconds
        #[arg(long, default_value_t = 10)]
        timeout: u64,
    },
    /// Check the app as it is now; prints a verdict with evidence (exit 1 if a check fails)
    ///
    /// CHECK: `visible TARGET`, `not visible TARGET`, `enabled|disabled|checked|unchecked|focused
    /// TARGET`, `text TARGET == VALUE`, `text TARGET ~= VALUE`, `screen ACTIVITY`, `no crash`,
    /// `log ~= TEXT`, `no log ~= TEXT`. `no crash` is always checked.
    Verify {
        checks: Vec<String>,
        /// Seconds screen checks may take to start holding
        #[arg(long, default_value_t = 3)]
        timeout: u64,
    },
    /// Recent logs and crash reports of the app in front
    Logs {
        /// Minimum level
        #[arg(long, value_enum, default_value_t = Level::Warn)]
        level: Level,
        /// Most recent lines to show
        #[arg(long, default_value_t = 50)]
        lines: usize,
    },
    /// Build the app, install it if it changed, restart it and show its first screen
    Run {
        /// Directory inside the Gradle project
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Application module such as `app` (when the build has several)
        #[arg(long)]
        module: Option<String>,
        /// Build variant such as `debug` or `freeDebug` (default: the debug variant)
        #[arg(long)]
        variant: Option<String>,
        /// Use the last built APK instead of building
        #[arg(long)]
        no_build: bool,
        /// Grant all runtime permissions on install
        #[arg(short, long)]
        grant: bool,
        /// Uninstall first (replaces an app signed with another key; clears its data)
        #[arg(long)]
        reinstall: bool,
    },
    /// What the uncommitted change reaches and what to verify there (no device needed)
    Impact {
        /// Directory inside the project
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Revision to compare the working tree with, e.g. `main` or `HEAD~1`
        #[arg(long, default_value = "HEAD")]
        base: String,
    },
    /// Launch an app by package (launcher activity) or package/activity
    Launch { app: String },
    /// Open a deep link, e.g. myapp://settings
    Open {
        uri: String,
        /// Only this app may handle it (default: the session's app, if any)
        #[arg(long)]
        package: Option<String>,
    },
    /// Device and app state: animations, permissions, app data
    State {
        #[command(subcommand)]
        command: StateCommand,
    },
    /// Force-stop an app
    Stop { package: String },
    /// Install an APK
    Install {
        apk: PathBuf,
        /// Grant all runtime permissions
        #[arg(short, long)]
        grant: bool,
    },
    /// Serve the tools over MCP on stdin/stdout (for agents)
    Mcp,
    /// Inspect or reset the session (refs, history, recorded steps)
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
    /// UI consistency: rule checks and structural baselines of the current screen
    Visual {
        #[command(subcommand)]
        command: VisualCommand,
    },
    /// Performance: startup time, frames, memory and CPU over repeated runs, against baselines
    /// and budgets; a Perfetto trace explains what's slow
    Perf {
        #[command(subcommand)]
        command: PerfCommand,
    },
    /// Compatibility: what the change puts at risk on other OS versions, device types, vendors and
    /// screen sizes, and a verdict per risk from the fewest devices and configurations that show it
    Compat {
        #[command(subcommand)]
        command: CompatCommand,
    },
    /// Save the session's recorded steps as a flow, list flows, replay them
    Flow {
        #[command(subcommand)]
        command: FlowCommand,
    },
}

#[derive(Subcommand)]
enum DevicesCommand {
    /// Use this device (serial, or an emulator's AVD name) by default in this project
    Use { device: String },
}

#[derive(Subcommand)]
enum EmulatorCommand {
    /// Start an emulator and wait until it has booted (default: the project's, else the newest)
    Start {
        avd: Option<String>,
        /// No window (CI, servers)
        #[arg(long)]
        headless: bool,
    },
    /// Shut an emulator down (default: the project's, else the only one running)
    Stop { device: Option<String> },
}

#[derive(Subcommand)]
enum VisualCommand {
    /// Check the current screen: rules (touch targets, labels, overlap, obscured, duplicate
    /// labels) and, with --baseline, its structural baseline (exit 1 on failure)
    Check {
        /// Compare with (or record) the baseline kept under this name
        #[arg(long)]
        baseline: Option<String>,
        /// Rules to check, comma-separated, or `all` / `none`
        #[arg(long, default_value = "all")]
        rules: String,
        /// Elements to leave out of the baseline comparison (dynamic content); repeatable
        #[arg(long)]
        ignore: Vec<String>,
        /// Also check the screen in these configurations, comma-separated: font_scale, dark,
        /// rtl, or all (each is switched on, checked and restored)
        #[arg(long, value_delimiter = ',')]
        configs: Vec<String>,
    },
    /// Make the candidates left by failed baseline comparisons the new baselines (all, or one
    /// flow's or name's)
    Approve { scope: Option<String> },
}

#[derive(Subcommand)]
enum PerfCommand {
    /// Cold start time of an app (with --hot, hot start too) over repeated runs (exit 1 on a
    /// regression)
    Startup {
        /// Package or package/activity (default: the session's app)
        app: Option<String>,
        #[arg(long)]
        hot: bool,
        /// Measured runs (one more runs first and is discarded)
        #[arg(long, default_value_t = 5)]
        runs: usize,
        /// Capture and explain a Perfetto trace even if nothing regressed
        #[arg(long)]
        trace: bool,
    },
    /// Frames, jank, memory and CPU while a saved flow runs, over repeated runs (exit 1 on a
    /// regression or a budget exceeded)
    Flow {
        name: String,
        /// Measured runs (default: the flow's `perf: runs`, else 5)
        #[arg(long)]
        runs: Option<usize>,
        #[arg(long)]
        trace: bool,
    },
    /// Summarize a trace `mdh perf` kept: what the main thread did, which frames were late
    Explain {
        trace: PathBuf,
        /// The app's package
        #[arg(long)]
        app: String,
        /// The trace is of a cold start (default: of a flow)
        #[arg(long)]
        startup: bool,
    },
    /// Make the latest measurements the baselines (all, or one scope's: a flow, `startup-<package>`)
    Approve { scope: Option<String> },
    /// Get Perfetto's trace processor, which explains traces (asks before downloading ~14 MB)
    Setup {
        /// The user agreed to the download
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum CompatCommand {
    /// The change's compatibility risks, with evidence and where each would show (no device)
    Risks {
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Revision to compare the working tree with
        #[arg(long, default_value = "HEAD")]
        base: String,
    },
    /// The cells that would verify the risks on what is connected, and what they cost
    Plan {
        #[arg(long, default_value = ".")]
        project: PathBuf,
        #[arg(long, default_value = "HEAD")]
        base: String,
        /// New emulators a run may start
        #[arg(long, default_value_t = 2)]
        max_emulators: usize,
    },
    /// Verify the risks: build once, run each cell's flows, a verdict per risk (exit 1 if one
    /// fails)
    Run {
        #[arg(long, default_value = ".")]
        project: PathBuf,
        #[arg(long, default_value = "HEAD")]
        base: String,
        /// The user agreed to start the emulators the plan needs
        #[arg(long)]
        yes: bool,
        /// Leave out cells that need an emulator started
        #[arg(long, conflicts_with = "yes")]
        no_start: bool,
        #[arg(long, default_value_t = 2)]
        max_emulators: usize,
        /// Seconds each step waits for its target
        #[arg(long, default_value_t = 10)]
        step_timeout: u64,
    },
}

#[derive(Subcommand)]
enum StateCommand {
    /// Turn system animations off (screens settle sooner) or back on; off is restored on session reset
    Animations { mode: OnOff },
    /// Grant a runtime permission, e.g. CAMERA or android.permission.POST_NOTIFICATIONS
    Grant {
        permission: String,
        /// Default: the session's app
        #[arg(long)]
        package: Option<String>,
    },
    /// Revoke a runtime permission
    Revoke {
        permission: String,
        #[arg(long)]
        package: Option<String>,
    },
    /// Clear the app's data (a first launch again)
    ClearData { package: Option<String> },
}

#[derive(Clone, Copy, ValueEnum)]
enum HookEvent {
    SessionStart,
    Stop,
}

#[derive(Clone, Copy, ValueEnum)]
enum OnOff {
    On,
    Off,
}

#[derive(Subcommand)]
enum FlowCommand {
    /// Save the recorded steps as .mdh/flows/NAME.yaml
    Save {
        name: String,
        /// Only the last N recorded steps
        #[arg(long)]
        last: Option<usize>,
        /// Checks to run after the last step (same syntax as `mdh verify`); repeatable
        #[arg(long = "check")]
        checks: Vec<String>,
        /// Overwrite an existing flow
        #[arg(long)]
        force: bool,
    },
    /// List saved flows
    List,
    /// Print a flow's YAML
    Show { name: String },
    /// Replay flows and print a verdict per flow (exit 1 if any fails)
    Run {
        #[arg(required_unless_present = "changed")]
        names: Vec<String>,
        /// Replay the flows that pass the screens the uncommitted change reaches (see `mdh impact`)
        #[arg(long, conflicts_with = "names")]
        changed: bool,
        /// With --changed: compare with this revision instead of HEAD
        #[arg(long, default_value = "HEAD", requires = "changed")]
        base: String,
        /// Write a JUnit report
        #[arg(long)]
        junit: Option<PathBuf>,
        /// Seconds each step waits for its target
        #[arg(long, default_value_t = 10)]
        step_timeout: u64,
        /// Seconds checks may take to start holding
        #[arg(long, default_value_t = 3)]
        timeout: u64,
    },
}

#[derive(Subcommand)]
enum SessionCommand {
    /// Show the session's device, last screen and recorded steps
    Show,
    /// Forget refs and steps and stop the device helper (frees UiAutomation for other tools)
    Reset,
}

#[derive(Clone, Copy, ValueEnum)]
enum Level {
    Verbose,
    Debug,
    Info,
    Warn,
    Error,
}

impl From<Level> for LogLevel {
    fn from(l: Level) -> Self {
        match l {
            Level::Verbose => LogLevel::Verbose,
            Level::Debug => LogLevel::Debug,
            Level::Info => LogLevel::Info,
            Level::Warn => LogLevel::Warn,
            Level::Error => LogLevel::Error,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl From<Dir> for Direction {
    fn from(d: Dir) -> Self {
        match d {
            Dir::Up => Direction::Up,
            Dir::Down => Direction::Down,
            Dir::Left => Direction::Left,
            Dir::Right => Direction::Right,
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let started = Instant::now();
    let mut timings = Timings::default();
    let json = cli.json;
    macro_rules! report {
        ($result:expr) => {{
            let (data, error) = split($result);
            finish(json, started, timings, data, error)
        }};
    }

    match cli.command {
        Command::Doctor => {
            let (checks, error) = doctor::run().await;
            finish(json, started, timings, Some(checks), error)
        }
        Command::Devices { command: None } => {
            report!(Inventory::load(Path::new(DEFAULT_DEVICE_FILE)).await)
        }
        Command::Devices {
            command: Some(DevicesCommand::Use { device }),
        } => report!(
            mdh_control::use_device(&device, Path::new(DEFAULT_DEVICE_FILE))
                .await
                .map(Done::new)
        ),
        Command::Emulator { command } => {
            let file = Path::new(DEFAULT_DEVICE_FILE);
            report!(match command {
                EmulatorCommand::Start { avd, headless } => {
                    if std::io::stderr().is_terminal() {
                        eprintln!("starting the emulator… (a cold boot can take a few minutes)");
                    }
                    mdh_control::start_emulator(avd.as_deref(), headless, file)
                        .await
                        .map(|(_, text)| Done::new(text))
                }
                EmulatorCommand::Stop { device } =>
                    mdh_control::stop_emulator(device.as_deref(), file)
                        .await
                        .map(Done::new),
            })
        }
        Command::Init {
            project,
            no_agents_md,
        } => report!(init(&project, !no_agents_md)),
        Command::Hook { event } => hook(event).await,
        Command::Impact { project, base } => {
            report!(
                mdh_impact::analyze(&mdh_impact::Options { project, base }).and_then(|mut r| {
                    r.verify.flows =
                        mdh_verify::flows_for(&r, &mdh_verify::FlowStore::new(FLOWS_DIR))?;
                    r.verify.compatibility = mdh_compat::summaries(&r);
                    Ok(r)
                })
            )
        }
        Command::Compat {
            command: CompatCommand::Risks { project, base },
        } => report!(mdh_compat::analyze(&project, &base)),
        Command::Visual {
            command: VisualCommand::Approve { scope },
        } => report!(mdh_visual::approve(scope.as_deref()).map(Done::new)),
        Command::Perf {
            command: PerfCommand::Approve { scope },
        } => report!(mdh_perf::approve(scope.as_deref()).map(Done::new)),
        Command::Perf {
            command:
                PerfCommand::Explain {
                    trace,
                    app,
                    startup,
                },
        } => {
            let scenario = if startup {
                mdh_perf::Scenario::Startup
            } else {
                mdh_perf::Scenario::Flow
            };
            report!(
                mdh_perf::explain(&trace, &app, scenario).map(|lines| Done::new(lines.join("\n")))
            )
        }
        Command::Perf {
            command: PerfCommand::Setup { yes },
        } => {
            let mut result = mdh_perf::setup(yes);
            if let Err(Error::NeedsConsent { action, .. }) = &result
                && std::io::stdin().is_terminal()
                && std::io::stderr().is_terminal()
            {
                eprint!("{}? [y/N] ", capitalized(action));
                let mut line = String::new();
                let agreed = std::io::stdin().read_line(&mut line).is_ok()
                    && matches!(line.trim().to_lowercase().as_str(), "y" | "yes");
                if agreed {
                    eprintln!("downloading…");
                    result = mdh_perf::setup(true);
                }
            }
            report!(result.map(Done::new))
        }
        Command::Flow {
            command: FlowCommand::List,
        } => report!(mdh_verify::FlowStore::new(FLOWS_DIR).list().map(Done::new)),
        Command::Flow {
            command: FlowCommand::Show { name },
        } => report!(
            mdh_verify::FlowStore::new(FLOWS_DIR)
                .load(&name)
                .map(|f| Done::new(f.to_yaml().trim_end()))
        ),
        Command::Mcp => match mdh_mcp::serve_stdio(cli.device).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: MCP server stopped: {e}");
                ExitCode::from(10)
            }
        },
        command => {
            let terminal = TerminalAsk;
            let interactive = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
            let connected = Control::connect_with(ConnectOptions {
                requested: cli.device.as_deref(),
                default_file: Path::new(DEFAULT_DEVICE_FILE),
                ask: interactive.then_some(&terminal as &dyn Ask),
                headless: false,
            })
            .await;
            let connected = match connected {
                Ok(c) => c,
                Err(e) => return finish::<Done>(json, started, timings, None, Some(e)),
            };
            // A freshly booted emulator shares nothing with the session kept for its serial.
            if connected.started {
                let _ = std::fs::remove_file(SESSION_FILE);
            }
            let mut session = Session::open(connected.control, Some(PathBuf::from(SESSION_FILE)));
            if let Some(notice) = connected.notice {
                session.notify(notice);
            }
            let code = run(command, &mut session, json, started, &mut timings).await;
            if let Err(e) = session.save() {
                eprintln!("warning: could not save the session to {SESSION_FILE}: {e}");
            }
            code
        }
    }
}

async fn run(
    command: Command,
    session: &mut Session,
    json: bool,
    started: Instant,
    timings: &mut Timings,
) -> ExitCode {
    macro_rules! report {
        ($result:expr) => {{
            let (data, error) = split($result);
            finish(json, started, std::mem::take(timings), data, error)
        }};
    }
    // Like `report!`, but a crash of the app turns into APP_CRASHED (exit 5) next to the data.
    macro_rules! report_crash {
        ($result:expr) => {{
            let (data, error) = split($result);
            let error = error.or_else(|| data.as_ref().and_then(crash_error));
            finish(json, started, std::mem::take(timings), data, error)
        }};
    }
    macro_rules! act {
        ($action:expr) => {
            report_crash!(match $action {
                Ok(action) => session.act(action, timings).await,
                Err(e) => Err(e),
            })
        };
    }
    let target = |s: &str| Target::parse(s);

    match command {
        Command::Doctor
        | Command::Devices { .. }
        | Command::Emulator { .. }
        | Command::Init { .. }
        | Command::Hook { .. }
        | Command::Impact { .. }
        | Command::Compat {
            command: CompatCommand::Risks { .. },
        }
        | Command::Mcp => {
            unreachable!("handled before connecting")
        }
        Command::Observe { diff } => report_crash!(session.observe(diff, timings).await),
        Command::Screenshot { output, max_edge } => report!(
            session
                .control()
                .screenshot(output, max_edge, timings)
                .await
        ),
        Command::Tap { target: t } => act!(target(&t).map(|target| Action::Tap { target })),
        Command::LongPress {
            target: t,
            duration_ms,
        } => act!(target(&t).map(|target| Action::LongPress {
            target,
            duration_ms
        })),
        Command::Type {
            text,
            into,
            append,
            enter,
        } => act!(
            into.as_deref()
                .map(target)
                .transpose()
                .map(|into| Action::Type {
                    text,
                    into,
                    append,
                    enter,
                })
        ),
        Command::Swipe {
            x1,
            y1,
            x2,
            y2,
            duration_ms,
        } => act!(Ok(Action::Swipe {
            from: (x1, y1),
            to: (x2, y2),
            duration_ms,
        })),
        Command::Scroll {
            direction,
            within,
            until,
        } => act!((|| -> Result<Action> {
            Ok(Action::Scroll {
                direction: direction.into(),
                within: within.as_deref().map(target).transpose()?,
                until: until.as_deref().map(target).transpose()?,
            })
        })()),
        Command::Key { name } => act!(Ok(Action::Key {
            name: name.to_uppercase()
        })),
        Command::Wait {
            target: t,
            gone,
            timeout,
        } => report_crash!(match target(&t) {
            Ok(t) =>
                session
                    .wait(&t, gone, Duration::from_secs(timeout), timings)
                    .await,
            Err(e) => Err(e),
        }),
        Command::Logs { level, lines } => report!(session.logs(level.into(), lines).await),
        Command::Verify { checks, timeout } => {
            let checks: Result<Vec<mdh_verify::Assertion>> = checks
                .iter()
                .map(|c| mdh_verify::Assertion::parse(c))
                .collect();
            let options = mdh_verify::VerifyOptions {
                timeout: Duration::from_secs(timeout),
                checks: check_kinds(),
                ..mdh_verify::VerifyOptions::default()
            };
            let verdict = match checks {
                Ok(checks) => mdh_verify::verify(session, checks, &options, timings).await,
                Err(e) => Err(e),
            };
            let (data, error) = split(verdict);
            let error = error.or_else(|| data.as_ref().and_then(mdh_verify::Verdict::failure));
            finish(json, started, std::mem::take(timings), data, error)
        }
        Command::Run {
            project,
            module,
            variant,
            no_build,
            grant,
            reinstall,
        } => {
            let options = RunOptions {
                project,
                module,
                variant,
                build: !no_build,
                grant,
                reinstall,
            };
            let report = session.run(options, timings, show_task).await;
            if std::io::stderr().is_terminal() {
                eprint!("\r\x1b[2K");
            }
            let (mut data, error) = split(report);
            let error = error
                .or_else(|| data.as_mut().and_then(RunReport::take_failure))
                .or_else(|| data.as_ref().and_then(crash_error));
            finish(json, started, std::mem::take(timings), data, error)
        }
        Command::Launch { app } => report!(session.launch(&app).await),
        Command::Open { uri, package } => {
            let package = package.or_else(|| session.app().map(str::to_owned));
            report!(session.open_uri(&uri, package.as_deref()).await)
        }
        Command::State { command } => report!(
            match command {
                StateCommand::Animations { mode } => {
                    session.animations(matches!(mode, OnOff::On)).await
                }
                StateCommand::Grant {
                    permission,
                    package,
                } =>
                    session
                        .set_permission(package.as_deref(), &permission, true)
                        .await,
                StateCommand::Revoke {
                    permission,
                    package,
                } =>
                    session
                        .set_permission(package.as_deref(), &permission, false)
                        .await,
                StateCommand::ClearData { package } => session.clear_data(package.as_deref()).await,
            }
            .map(Done::new)
        ),
        Command::Stop { package } => report!(
            session
                .control()
                .stop(&package)
                .await
                .map(|()| Done::new(format!("stopped {package}")))
        ),
        Command::Install { apk, grant } => report!(
            session
                .control()
                .install(&apk, grant)
                .await
                .map(|()| Done::new(format!("installed {}", apk.display())))
        ),
        Command::Visual { command } => match command {
            VisualCommand::Check {
                baseline,
                rules,
                ignore,
                configs,
            } => {
                let rules: serde_json::Value = match rules.as_str() {
                    "all" | "none" => rules.clone().into(),
                    list => list
                        .split(',')
                        .map(|r| r.trim().to_owned())
                        .collect::<Vec<_>>()
                        .into(),
                };
                let options = mdh_verify::VerifyOptions {
                    checks: check_kinds(),
                    config: Some(serde_json::json!({
                        "rules": rules,
                        "baseline": baseline.is_some(),
                        "ignore": ignore,
                        "configs": configs,
                    })),
                    scope: baseline,
                    ..mdh_verify::VerifyOptions::default()
                };
                let verdict = mdh_verify::verify(session, Vec::new(), &options, timings).await;
                let (data, error) = split(verdict);
                let error = error.or_else(|| data.as_ref().and_then(mdh_verify::Verdict::failure));
                finish(json, started, std::mem::take(timings), data, error)
            }
            VisualCommand::Approve { .. } => unreachable!("handled before connecting"),
        },
        Command::Compat { command } => match command {
            CompatCommand::Plan {
                project,
                base,
                max_emulators,
            } => {
                let options = mdh_compat::CompatOptions {
                    project,
                    base,
                    plan: mdh_compat::PlanOptions {
                        max_starts: max_emulators,
                    },
                    ..mdh_compat::CompatOptions::default()
                };
                report!(mdh_compat::plan_for(session, &options).await)
            }
            CompatCommand::Run {
                project,
                base,
                yes,
                no_start,
                max_emulators,
                step_timeout,
            } => {
                let options = mdh_compat::CompatOptions {
                    project,
                    base,
                    consent: yes,
                    no_start,
                    plan: mdh_compat::PlanOptions {
                        max_starts: max_emulators,
                    },
                    step_timeout: Duration::from_secs(step_timeout),
                    ..mdh_compat::CompatOptions::default()
                };
                let mut result = mdh_compat::run(session, &options, timings).await;
                if let Err(Error::NeedsConsent { action, .. }) = &result
                    && std::io::stdin().is_terminal()
                    && std::io::stderr().is_terminal()
                {
                    eprint!("{}? [y/N] ", capitalized(action));
                    let mut line = String::new();
                    if std::io::stdin().read_line(&mut line).is_ok()
                        && matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
                    {
                        let options = mdh_compat::CompatOptions {
                            consent: true,
                            ..options.clone()
                        };
                        result = mdh_compat::run(session, &options, timings).await;
                    }
                }
                let (data, error) = split(result);
                let error =
                    error.or_else(|| data.as_ref().and_then(mdh_compat::CompatReport::failure));
                finish(json, started, std::mem::take(timings), data, error)
            }
            CompatCommand::Risks { .. } => unreachable!("handled before connecting"),
        },
        Command::Perf { command } => {
            let verdict = match command {
                PerfCommand::Startup {
                    app,
                    hot,
                    runs,
                    trace,
                } => {
                    let options = mdh_perf::PerfOptions {
                        runs: runs.max(1),
                        trace,
                        ..mdh_perf::PerfOptions::default()
                    };
                    match session.package_or_app(app.as_deref()) {
                        Ok(app) => mdh_perf::startup(session, &app, hot, &options).await,
                        Err(e) => Err(e),
                    }
                }
                PerfCommand::Flow { name, runs, trace } => {
                    let options = mdh_perf::PerfOptions {
                        runs: runs.unwrap_or(5).max(1),
                        trace,
                        ..mdh_perf::PerfOptions::default()
                    };
                    match mdh_verify::FlowStore::new(FLOWS_DIR).load(&name) {
                        Ok(mut flow) => {
                            if let Some(runs) = runs
                                && let Some(perf) =
                                    flow.perf.as_mut().and_then(|p| p.as_object_mut())
                            {
                                perf.insert("runs".into(), runs.into());
                            }
                            mdh_perf::flow(session, &flow, &options, timings).await
                        }
                        Err(e) => Err(e),
                    }
                }
                PerfCommand::Approve { .. }
                | PerfCommand::Setup { .. }
                | PerfCommand::Explain { .. } => {
                    unreachable!("handled before connecting")
                }
            };
            let (data, error) = split(verdict);
            let error = error.or_else(|| data.as_ref().and_then(mdh_verify::Verdict::failure));
            finish(json, started, std::mem::take(timings), data, error)
        }
        Command::Flow { command } => match command {
            FlowCommand::Save {
                name,
                last,
                checks,
                force,
            } => report!(
                mdh_verify::save_flow(
                    session,
                    &mdh_verify::FlowStore::new(FLOWS_DIR),
                    &name,
                    last,
                    &checks,
                    force
                )
                .map(|saved| Done::new(saved.text))
            ),
            FlowCommand::Run {
                names,
                changed,
                base,
                junit,
                step_timeout,
                timeout,
            } => {
                let options = mdh_verify::FlowOptions {
                    step_timeout: Duration::from_secs(step_timeout),
                    verify: mdh_verify::VerifyOptions {
                        timeout: Duration::from_secs(timeout),
                        checks: check_kinds(),
                        ..mdh_verify::VerifyOptions::default()
                    },
                };
                let store = mdh_verify::FlowStore::new(FLOWS_DIR);
                let runs = if changed {
                    mdh_verify::run_changed(
                        session,
                        &store,
                        Path::new("."),
                        &base,
                        &options,
                        timings,
                    )
                    .await
                } else {
                    mdh_verify::run_flows(session, &store, &names, &options, timings).await
                };
                let runs = runs.and_then(|runs| {
                    if let Some(path) = &junit {
                        std::fs::write(path, mdh_verify::junit("mdh", &runs.verdicts))?;
                    }
                    Ok(runs)
                });
                let (data, error) = split(runs);
                let error = error.or_else(|| data.as_ref().and_then(mdh_verify::FlowRuns::failure));
                finish(json, started, std::mem::take(timings), data, error)
            }
            FlowCommand::List | FlowCommand::Show { .. } => {
                unreachable!("handled before connecting")
            }
        },
        Command::Session { command } => match command {
            SessionCommand::Show => report!(Ok(session.summary())),
            SessionCommand::Reset => report!(
                session
                    .reset()
                    .await
                    .map(|()| Done::new("session reset; device helper stopped"))
            ),
        },
    }
}

/// Shows the running Gradle task on an interactive terminal; silent otherwise.
fn show_task(task: &str) {
    if std::io::stderr().is_terminal() {
        let task: String = task.chars().take(70).collect();
        eprint!("\r\x1b[2Kbuilding {task}");
    }
}

trait HasLogs {
    fn logs(&self) -> Option<&LogDigest>;
}

impl HasLogs for RunReport {
    fn logs(&self) -> Option<&LogDigest> {
        self.observation.as_ref()?.logs.as_ref()
    }
}

impl HasLogs for Observation {
    fn logs(&self) -> Option<&LogDigest> {
        self.logs.as_ref()
    }
}

impl HasLogs for ActOutcome {
    fn logs(&self) -> Option<&LogDigest> {
        self.logs.as_ref()
    }
}

/// The first crash or ANR of the app itself; deaths without a report and other apps' crashes
/// are shown but don't fail the command.
fn crash_error(data: &impl HasLogs) -> Option<Error> {
    data.logs()?
        .crashes
        .iter()
        .find(|c| c.of_app && c.kind != CrashKind::Died)
        .map(|c| Error::AppCrashed {
            package: c.package.clone().unwrap_or_else(|| "the app".into()),
            summary: c.summary.clone(),
        })
}

fn split<T>(result: Result<T>) -> (Option<T>, Option<mdh_core::Error>) {
    match result {
        Ok(data) => (Some(data), None),
        Err(e) => (None, Some(e)),
    }
}

/// Asks at the terminal when no device can be picked alone.
struct TerminalAsk;

impl TerminalAsk {
    fn read(prompt: &str, max: usize, default: Option<usize>) -> Option<usize> {
        eprint!("{prompt}");
        let mut line = String::new();
        // End of input is no answer, not consent.
        if std::io::stdin().read_line(&mut line).ok()? == 0 {
            return None;
        }
        let answer = line.trim().to_lowercase();
        match answer.as_str() {
            "" => default,
            "y" | "yes" => default,
            "n" | "no" | "q" => None,
            n => n
                .parse::<usize>()
                .ok()
                .filter(|i| (1..=max).contains(i))
                .map(|i| i - 1),
        }
    }
}

impl Ask for TerminalAsk {
    fn choose(&self, devices: &[Device]) -> Option<usize> {
        eprintln!("Several devices are connected:");
        for (i, d) in devices.iter().enumerate() {
            eprintln!("  [{}] {}", i + 1, d.describe());
        }
        Self::read(
            &format!("Use which one for this project? [1-{}] ", devices.len()),
            devices.len(),
            None,
        )
    }

    fn start(&self, avds: &[Avd]) -> Option<usize> {
        eprintln!("No device is connected. Emulators you can start:");
        for (i, a) in avds.iter().enumerate() {
            eprintln!("  [{}] {}", i + 1, a.describe());
        }
        Self::read(
            &format!("Start {}? [Y/n, or 1-{}] ", avds[0].name, avds.len()),
            avds.len(),
            Some(0),
        )
    }

    fn starting(&self, avd: &str) {
        eprintln!("starting {avd}… (a cold boot can take a few minutes)");
    }
}

impl Human for Inventory {
    fn human(&self) -> String {
        self.text()
    }
}

fn init(project: &Path, agents_md: bool) -> Result<Done> {
    let dir = project.canonicalize()?;
    let root = dir
        .ancestors()
        .find(|d| d.join("settings.gradle.kts").is_file() || d.join("settings.gradle").is_file())
        .unwrap_or(&dir);
    let done = mdh_verify::agent::init(root, agents_md)?;
    Ok(Done::new(if done.is_empty() {
        "already set up".to_owned()
    } else {
        done.join("\n")
    }))
}

/// Claude Code hook handlers. They never fail the session: on any problem they stay silent.
async fn hook(event: HookEvent) -> ExitCode {
    let mut input = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut input);
    let input: serde_json::Value = serde_json::from_str(&input).unwrap_or_default();
    let cwd = input["cwd"]
        .as_str()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let gradle_root = cwd
        .ancestors()
        .find(|d| d.join("settings.gradle.kts").is_file() || d.join("settings.gradle").is_file())
        .map(Path::to_path_buf);
    let Some(root) = gradle_root else {
        return ExitCode::SUCCESS;
    };
    match event {
        HookEvent::SessionStart => {
            let devices = match Inventory::load(&cwd.join(DEFAULT_DEVICE_FILE)).await {
                Ok(inv) => {
                    let online: Vec<String> = inv
                        .devices
                        .iter()
                        .filter(|d| d.state == DeviceState::Online)
                        .map(Device::describe)
                        .collect();
                    let avds: Vec<String> = inv.avds.iter().map(Avd::describe).collect();
                    match (online.is_empty(), avds.is_empty()) {
                        (false, _) => format!("devices online: {}", online.join(", ")),
                        (true, false) => format!(
                            "no device online; emulators that can be started: {} — ask the user before \
                             starting one (mdh_status with start_emulator, or `mdh emulator start`)",
                            avds.join(", ")
                        ),
                        (true, true) => "no device online and no emulator set up".to_owned(),
                    }
                }
                Err(e) => format!("devices unknown ({e})"),
            };
            let flows = mdh_verify::FlowStore::new(root.join(".mdh/flows"))
                .names()
                .map(|n| n.len())
                .unwrap_or(0);
            let context = format!(
                "This is an Android project verified with mobile-dev-harness (mdh): a change is done \
                 when it has a passing verdict (see the `verify` skill). {devices}; saved flows: {flows}."
            );
            let out = serde_json::json!({
                "hookSpecificOutput": { "hookEventName": "SessionStart", "additionalContext": context }
            });
            println!("{out}");
        }
        HookEvent::Stop => {
            // Already continuing because of this hook: one reminder is enough.
            if input["stop_hook_active"].as_bool() == Some(true) {
                return ExitCode::SUCCESS;
            }
            let since = input["transcript_path"]
                .as_str()
                .and_then(|p| std::fs::metadata(p).ok()?.created().ok());
            if let Some(reason) = mdh_verify::agent::stop_reason(&cwd, since) {
                println!(
                    "{}",
                    serde_json::json!({ "decision": "block", "reason": reason })
                );
            }
        }
    }
    ExitCode::SUCCESS
}

fn capitalized(s: &str) -> String {
    let mut chars = s.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// Check kinds that run next to the functional checks in every verification.
fn check_kinds() -> Vec<std::sync::Arc<dyn mdh_verify::Check>> {
    vec![std::sync::Arc::new(mdh_visual::Visual::default())]
}
