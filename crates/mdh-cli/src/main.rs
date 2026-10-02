mod doctor;

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use mdh_core::DeviceState;
use mdh_driver::Driver;
use mdh_driver::android::{AndroidDriver, AndroidSdk};

#[derive(Parser)]
#[command(name = "mdh", version, about)]
struct Cli {
    /// Emit machine-readable JSON instead of human-readable text
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check that the local toolchain (SDK, adb, emulator, JDK) is ready
    Doctor,
    /// List connected devices and running emulators
    Devices,
}

#[tokio::main]
async fn main() -> anyhow::Result<ExitCode> {
    let cli = Cli::parse();
    match cli.command {
        Command::Doctor => doctor::run(cli.json).await,
        Command::Devices => devices(cli.json).await,
    }
}

async fn devices(json: bool) -> anyhow::Result<ExitCode> {
    let sdk = AndroidSdk::locate()?;
    let devices = AndroidDriver::new(&sdk).devices().await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&devices)?);
    } else if devices.is_empty() {
        println!("No devices connected. Start an emulator or plug in a device.");
    } else {
        for d in &devices {
            let state = match &d.state {
                DeviceState::Online => "online",
                DeviceState::Offline => "offline",
                DeviceState::Unauthorized => "unauthorized (accept the USB debugging prompt)",
                DeviceState::Other(s) => s,
            };
            let kind = if d.is_emulator { "emulator" } else { "device" };
            let model = d.model.as_deref().unwrap_or("-");
            println!("{:<20} {:<9} {:<24} {}", d.id, kind, model, state);
        }
    }
    Ok(ExitCode::SUCCESS)
}
