//! Device drivers for mobile-dev-harness.

pub mod android;
mod process;

use std::path::{Path, PathBuf};

use std::time::Duration;

use async_trait::async_trait;
use mdh_core::ui::RawTree;
use mdh_core::{Device, DeviceState, Error, Input, LaunchInfo, LogEntry, Platform, Result};

/// A platform backend: low-level device capabilities only. Element targeting, waiting and
/// observation live above it and are shared by all backends.
#[async_trait]
pub trait Driver: Send + Sync {
    fn platform(&self) -> Platform;

    async fn devices(&self) -> Result<Vec<Device>>;

    async fn ui_tree(&self, device: &Device) -> Result<RawTree>;

    /// The focused activity, e.g. `com.example/.MainActivity`.
    async fn foreground_activity(&self, device: &Device) -> Result<Option<String>>;

    async fn input(&self, device: &Device, input: &Input) -> Result<()>;

    /// A PNG of the screen.
    async fn screenshot(&self, device: &Device) -> Result<Vec<u8>>;

    async fn install(&self, device: &Device, app: &Path, grant_permissions: bool) -> Result<()>;

    /// Starts `app`, a package (its launcher activity) or a `package/activity` component.
    async fn launch(&self, device: &Device, app: &str) -> Result<LaunchInfo>;

    async fn stop(&self, device: &Device, package: &str) -> Result<()>;

    /// Where `package`'s APK lives on the device, `None` when it isn't installed. Every install
    /// gets a new path, so it also tells whether someone else reinstalled the app.
    async fn installed_path(&self, _device: &Device, _package: &str) -> Result<Option<String>> {
        Ok(None)
    }

    async fn uninstall(&self, _device: &Device, _package: &str) -> Result<()> {
        Ok(())
    }

    /// Opens a deep link (a `VIEW` intent for `uri`), limited to `package` if given.
    async fn open_uri(
        &self,
        _device: &Device,
        _uri: &str,
        _package: Option<&str>,
    ) -> Result<LaunchInfo> {
        Err(unsupported("opening deep links"))
    }

    /// Clears the app's data and stops it: a first launch again.
    async fn clear_data(&self, _device: &Device, _package: &str) -> Result<()> {
        Err(unsupported("clearing app data"))
    }

    /// Grants or revokes a runtime permission.
    async fn set_permission(
        &self,
        _device: &Device,
        _package: &str,
        _permission: &str,
        _granted: bool,
    ) -> Result<()> {
        Err(unsupported("changing permissions"))
    }

    /// Supported ABIs, preferred first (e.g. `arm64-v8a`); empty when unknown.
    async fn abis(&self, _device: &Device) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    /// The platform SDK the driver uses, so builds can be pointed at the same one.
    fn sdk_root(&self) -> Option<PathBuf> {
        None
    }

    /// The device clock in Unix milliseconds. Log cursors use it, never the host clock.
    async fn clock_ms(&self, _device: &Device) -> Result<u64> {
        Ok(0)
    }

    /// Log entries newer than `since_ms` (device clock), oldest first; capped to the most recent
    /// few thousand.
    async fn logs(&self, _device: &Device, _since_ms: u64) -> Result<Vec<LogEntry>> {
        Ok(Vec::new())
    }

    /// Running process ids of the given packages.
    async fn pids(&self, _device: &Device, _packages: &[String]) -> Result<Vec<u32>> {
        Ok(Vec::new())
    }

    /// Stops anything the driver keeps running on the device (e.g. the helper holding UiAutomation).
    async fn release(&self, _device: &Device) -> Result<()> {
        Ok(())
    }

    /// Waits until the UI produced no events for `quiet`; `false` if `timeout` hit first or the
    /// backend can't tell. Not sufficient on its own right after an action (events are throttled),
    /// see `mdh_control`'s settle logic.
    async fn wait_idle(
        &self,
        _device: &Device,
        _quiet: Duration,
        _timeout: Duration,
    ) -> Result<bool> {
        Ok(false)
    }
}

/// Picks the device to work on: the requested one, or the only online device.
fn unsupported(operation: &str) -> Error {
    Error::Unsupported {
        operation: operation.to_owned(),
    }
}

pub fn select_device(devices: Vec<Device>, requested: Option<&str>) -> Result<Device> {
    let mut online: Vec<Device> = devices
        .into_iter()
        .filter(|d| d.state == DeviceState::Online)
        .collect();
    if let Some(id) = requested {
        return online
            .into_iter()
            .find(|d| d.id == id)
            .ok_or_else(|| Error::DeviceNotFound { id: id.to_owned() });
    }
    match online.len() {
        0 => Err(Error::NoDevice),
        1 => Ok(online.remove(0)),
        _ => Err(Error::AmbiguousDevice {
            candidates: online.into_iter().map(|d| d.id).collect(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: &str, state: DeviceState) -> Device {
        Device {
            id: id.into(),
            platform: Platform::Android,
            state,
            model: None,
            is_emulator: true,
        }
    }

    #[test]
    fn selects_the_only_online_device() {
        let devices = vec![
            device("a", DeviceState::Offline),
            device("b", DeviceState::Online),
        ];
        assert_eq!(select_device(devices, None).unwrap().id, "b");
    }

    #[test]
    fn refuses_to_guess_between_devices() {
        let devices = vec![
            device("a", DeviceState::Online),
            device("b", DeviceState::Online),
        ];
        assert!(matches!(
            select_device(devices.clone(), None),
            Err(Error::AmbiguousDevice { .. })
        ));
        assert_eq!(select_device(devices, Some("a")).unwrap().id, "a");
    }

    #[test]
    fn requested_device_must_be_online() {
        let devices = vec![device("a", DeviceState::Unauthorized)];
        assert!(matches!(
            select_device(devices, Some("a")),
            Err(Error::DeviceNotFound { .. })
        ));
    }
}
