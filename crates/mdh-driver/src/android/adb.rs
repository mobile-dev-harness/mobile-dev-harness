use std::path::{Path, PathBuf};
use std::time::Duration;

use mdh_core::{Device, DeviceState, Platform, Result};

use crate::process::{run, run_capture, run_with_timeout};

pub struct Adb {
    path: PathBuf,
}

impl Adb {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// First line of `adb version`, e.g. `Android Debug Bridge version 1.0.41`.
    pub async fn version(&self) -> Result<String> {
        let out = run(&self.path, &["version"]).await?;
        Ok(out.lines().next().unwrap_or_default().trim().to_owned())
    }

    pub async fn devices(&self) -> Result<Vec<Device>> {
        let out = run(&self.path, &["devices", "-l"]).await?;
        Ok(parse_devices(&out))
    }

    /// `adb -s <serial> <args...>`.
    pub async fn on(&self, serial: &str, args: &[&str]) -> Result<String> {
        self.on_with_timeout(serial, args, Duration::from_secs(120))
            .await
    }

    pub async fn on_with_timeout(
        &self,
        serial: &str,
        args: &[&str],
        timeout: Duration,
    ) -> Result<String> {
        let mut full = vec!["-s", serial];
        full.extend_from_slice(args);
        run_with_timeout(&self.path, &full, timeout).await
    }

    /// Runs one shell command line on the device. adb propagates the remote exit status.
    pub async fn shell(&self, serial: &str, command: &str) -> Result<String> {
        self.on(serial, &["shell", command]).await
    }

    /// Like [`shell`](Self::shell) but returns stdout even when the command fails.
    pub async fn shell_stdout(&self, serial: &str, command: &str) -> Result<String> {
        let args = ["-s", serial, "shell", command];
        Ok(run_capture(&self.path, &args, Duration::from_secs(120))
            .await?
            .0)
    }

    /// Raw stdout bytes of `adb exec-out`, for binary output such as screenshots.
    pub async fn exec_out_bytes(&self, serial: &str, args: &[&str]) -> Result<Vec<u8>> {
        let mut full = vec!["-s", serial, "exec-out"];
        full.extend_from_slice(args);
        crate::process::run_bytes(&self.path, &full, Duration::from_secs(30)).await
    }
}

/// Single-quotes `s` for the device shell, so values like `pkg/.Main$Inner` reach the command
/// unchanged.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Parses `adb devices -l`. Lines look like
/// `emulator-5554  device product:sdk_gphone64_arm64 model:sdk_gphone64_arm64 transport_id:1`.
fn parse_devices(out: &str) -> Vec<Device> {
    out.lines()
        .filter(|l| !l.starts_with("List of devices") && !l.starts_with('*'))
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let id = fields.next()?;
            let state = match fields.next()? {
                "device" => DeviceState::Online,
                "offline" => DeviceState::Offline,
                "unauthorized" => DeviceState::Unauthorized,
                other => DeviceState::Other(other.to_owned()),
            };
            let model = fields
                .find_map(|f| f.strip_prefix("model:"))
                .map(str::to_owned);
            Some(Device {
                id: id.to_owned(),
                platform: Platform::Android,
                state,
                model,
                is_emulator: id.starts_with("emulator-"),
                avd: None,
                api: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_devices_handles_mixed_states() {
        let out = "\
* daemon not running; starting now at tcp:5037
* daemon started successfully
List of devices attached
emulator-5554          device product:sdk_gphone64_arm64 model:sdk_gphone64_arm64 device:emu64a transport_id:1
R58M123ABC             unauthorized usb:1-1 transport_id:2

";
        let devices = parse_devices(out);
        assert_eq!(devices.len(), 2);

        assert_eq!(devices[0].id, "emulator-5554");
        assert_eq!(devices[0].state, DeviceState::Online);
        assert_eq!(devices[0].model.as_deref(), Some("sdk_gphone64_arm64"));
        assert!(devices[0].is_emulator);

        assert_eq!(devices[1].state, DeviceState::Unauthorized);
        assert_eq!(devices[1].model, None);
        assert!(!devices[1].is_emulator);
    }

    #[test]
    fn shell_quote_keeps_metacharacters_literal() {
        assert_eq!(shell_quote("a/.B$C"), "'a/.B$C'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn parse_devices_empty() {
        assert!(parse_devices("List of devices attached\n\n").is_empty());
    }
}
