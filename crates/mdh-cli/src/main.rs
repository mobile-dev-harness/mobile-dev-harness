mod doctor;
mod output;
mod render;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand, ValueEnum};
use mdh_control::{ActOutcome, Action, Control, Direction, Observation, Session, Target};
use mdh_core::output::Timings;
use mdh_core::{Device, DeviceState, Error, LogLevel, Result};
use mdh_driver::Driver;
use mdh_driver::android::{AndroidDriver, AndroidSdk};
use mdh_observe::{CrashKind, LogDigest};
use serde::Serialize;

use crate::output::{Human, finish};
use crate::render::Done;

/// Where CLI invocations keep session state between calls, relative to the working directory.
const SESSION_FILE: &str = ".mdh/session.json";

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
    /// List connected devices and running emulators
    Devices,
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
    /// Recent logs and crash reports of the app in front
    Logs {
        /// Minimum level
        #[arg(long, value_enum, default_value_t = Level::Warn)]
        level: Level,
        /// Most recent lines to show
        #[arg(long, default_value_t = 50)]
        lines: usize,
    },
    /// Launch an app by package (launcher activity) or package/activity
    Launch { app: String },
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
        Command::Devices => report!(devices().await),
        Command::Mcp => match mdh_mcp::serve_stdio(cli.device).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: MCP server stopped: {e}");
                ExitCode::from(10)
            }
        },
        command => {
            let control = match Control::connect(cli.device.as_deref()).await {
                Ok(control) => control,
                Err(e) => return finish::<Done>(json, started, timings, None, Some(e)),
            };
            let mut session = Session::open(control, Some(PathBuf::from(SESSION_FILE)));
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
        Command::Doctor | Command::Devices | Command::Mcp => {
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
        Command::Launch { app } => report!(session.launch(&app).await),
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

trait HasLogs {
    fn logs(&self) -> Option<&LogDigest>;
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

#[derive(Serialize)]
#[serde(transparent)]
struct DeviceList(Vec<Device>);

impl Human for DeviceList {
    fn human(&self) -> String {
        if self.0.is_empty() {
            return "No devices connected. Start an emulator or plug in a device.".into();
        }
        self.0
            .iter()
            .map(|d| {
                let state = match &d.state {
                    DeviceState::Online => "online",
                    DeviceState::Offline => "offline",
                    DeviceState::Unauthorized => "unauthorized (accept the USB debugging prompt)",
                    DeviceState::Other(s) => s,
                };
                let kind = if d.is_emulator { "emulator" } else { "device" };
                let model = d.model.as_deref().unwrap_or("-");
                format!("{:<20} {:<9} {:<24} {}", d.id, kind, model, state)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

async fn devices() -> Result<DeviceList> {
    let sdk = AndroidSdk::locate()?;
    Ok(DeviceList(AndroidDriver::new(&sdk).devices().await?))
}
