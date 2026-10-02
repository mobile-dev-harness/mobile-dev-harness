mod act;
mod context;
mod doctor;
mod observe;
mod output;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use clap::{Parser, Subcommand};
use mdh_core::{Device, DeviceState, Input, Result};
use mdh_driver::Driver;
use mdh_driver::android::{AndroidDriver, AndroidSdk};
use serde::Serialize;

use crate::context::Context;
use crate::output::{Human, Phases, finish};

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

#[derive(Subcommand)]
enum Command {
    /// Check that the local toolchain (SDK, adb, emulator, JDK) is ready
    Doctor,
    /// List connected devices and running emulators
    Devices,
    /// Show the current screen as a compact UI tree
    Observe,
    /// Save a downscaled JPEG screenshot
    Screenshot {
        #[arg(short, long, default_value = "screenshot.jpg")]
        output: PathBuf,
        /// Longest edge in pixels
        #[arg(long, default_value_t = 1024)]
        max_edge: u32,
    },
    /// Tap at device coordinates
    Tap { x: i32, y: i32 },
    /// Swipe between device coordinates
    Swipe {
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        #[arg(long, default_value_t = 300)]
        duration_ms: u32,
    },
    /// Press a key, e.g. BACK, HOME, ENTER
    Key { name: String },
    /// Replace the focused text field's content (any Unicode text)
    Type { text: String },
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
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let started = Instant::now();
    let mut phases = Phases::new();
    let json = cli.json;
    macro_rules! report {
        ($result:expr) => {{
            let (data, error) = split($result);
            finish(json, started, phases, data, error)
        }};
    }

    match cli.command {
        Command::Doctor => {
            let (checks, error) = doctor::run().await;
            finish(json, started, phases, Some(checks), error)
        }
        Command::Devices => report!(devices().await),
        command => {
            let cx = match Context::new(cli.device.as_deref()).await {
                Ok(cx) => cx,
                Err(e) => return finish::<act::Done>(json, started, phases, None, Some(e)),
            };
            match command {
                Command::Doctor | Command::Devices => unreachable!("handled above"),
                Command::Observe => report!(observe::observe(&cx, &mut phases).await),
                Command::Screenshot { output, max_edge } => {
                    report!(observe::screenshot(&cx, output, max_edge, &mut phases).await)
                }
                Command::Tap { x, y } => {
                    report!(act::input(&cx, Input::Tap { x, y }, format!("tapped {x},{y}")).await)
                }
                Command::Swipe {
                    x1,
                    y1,
                    x2,
                    y2,
                    duration_ms,
                } => report!(
                    act::input(
                        &cx,
                        Input::Swipe {
                            from: (x1, y1),
                            to: (x2, y2),
                            duration_ms,
                        },
                        format!("swiped {x1},{y1} → {x2},{y2}"),
                    )
                    .await
                ),
                Command::Key { name } => {
                    let name = name.to_uppercase();
                    let describe = format!("pressed {name}");
                    report!(act::input(&cx, Input::Key { name }, describe).await)
                }
                Command::Type { text } => {
                    let describe = format!("typed {text:?}");
                    report!(act::input(&cx, Input::SetText { text }, describe).await)
                }
                Command::Launch { app } => report!(act::launch(&cx, &app).await),
                Command::Stop { package } => report!(act::stop(&cx, &package).await),
                Command::Install { apk, grant } => report!(act::install(&cx, &apk, grant).await),
            }
        }
    }
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
