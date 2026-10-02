mod doctor;
mod observe;
mod output;

use std::process::ExitCode;
use std::time::Instant;

use clap::{Parser, Subcommand};
use mdh_core::{Device, DeviceState, Result};
use mdh_driver::Driver;
use mdh_driver::android::{AndroidDriver, AndroidSdk};
use serde::Serialize;

use crate::output::{Human, finish};

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
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let started = Instant::now();
    match cli.command {
        Command::Doctor => {
            let (checks, error) = doctor::run().await;
            finish(cli.json, started, Vec::new(), Some(checks), error)
        }
        Command::Devices => {
            let (data, error) = split(devices().await);
            finish(cli.json, started, Vec::new(), data, error)
        }
        Command::Observe => {
            let mut phases = Vec::new();
            let (data, error) = split(observe::run(cli.device.as_deref(), &mut phases).await);
            finish(cli.json, started, phases, data, error)
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
