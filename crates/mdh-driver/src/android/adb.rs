use std::path::{Path, PathBuf};

use mdh_core::{Device, DeviceState, Platform, Result};

use crate::process::run;

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
    fn parse_devices_empty() {
        assert!(parse_devices("List of devices attached\n\n").is_empty());
    }
}
