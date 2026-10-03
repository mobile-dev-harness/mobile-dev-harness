//! Device drivers for mobile-dev-harness.

pub mod android;
mod process;

use std::path::{Path, PathBuf};

use std::time::Duration;

use async_trait::async_trait;
use mdh_core::ui::RawTree;
use mdh_core::{
    Appearance, AppearanceKind, Avd, Device, Error, FrameStats, Input, LaunchInfo, LogEntry,
    MemoryStats, PhysicalDisplay, Platform, Result,
};

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

    /// Virtual devices that can be started, and which of them run.
    async fn avds(&self) -> Result<Vec<Avd>> {
        Ok(Vec::new())
    }

    /// Starts the virtual device `avd` and waits until it has booted; `headless` shows no window.
    async fn start_emulator(&self, avd: &str, _headless: bool) -> Result<Device> {
        Err(Error::EmulatorFailed {
            avd: avd.to_owned(),
            reason: "this driver can't start emulators".into(),
        })
    }

    /// Shuts an emulator down.
    async fn stop_emulator(&self, _device: &Device) -> Result<()> {
        Err(unsupported("stopping emulators"))
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

    /// Screen density in dots per inch (160 = 1 px per dp), to measure layouts in dp.
    async fn density(&self, _device: &Device) -> Result<u32> {
        Err(unsupported("reading the screen density"))
    }

    /// The system's animation scales by name, `None` where unset; what to restore later.
    async fn animation_scales(&self, _device: &Device) -> Result<Vec<(String, Option<String>)>> {
        Err(unsupported("reading animation settings"))
    }

    /// Sets animation scales (`None` resets one to the system default).
    async fn set_animation_scales(
        &self,
        _device: &Device,
        _scales: &[(String, Option<String>)],
    ) -> Result<()> {
        Err(unsupported("changing animation settings"))
    }

    /// Frame timing of `package` since the last reset; with `reset`, starts counting anew.
    async fn frame_stats(
        &self,
        _device: &Device,
        _package: &str,
        _reset: bool,
    ) -> Result<FrameStats> {
        Err(unsupported("reading frame statistics"))
    }

    async fn memory(&self, _device: &Device, _package: &str) -> Result<MemoryStats> {
        Err(unsupported("reading memory statistics"))
    }

    /// CPU time (user plus system) a process has used, in milliseconds.
    async fn cpu_time_ms(&self, _device: &Device, _pid: u32) -> Result<u64> {
        Err(unsupported("reading CPU time"))
    }

    /// Whether the installed app is a debug build (slower; its numbers only compare with other
    /// debug builds).
    async fn debuggable(&self, _device: &Device, _package: &str) -> Result<bool> {
        Ok(false)
    }

    /// Starts a system trace with `config` (Perfetto text format) in the background.
    async fn start_trace(&self, _device: &Device, _config: &str) -> Result<()> {
        Err(unsupported("system tracing"))
    }

    /// Stops the trace started by `start_trace` and returns it.
    async fn stop_trace(&self, _device: &Device) -> Result<Vec<u8>> {
        Err(unsupported("system tracing"))
    }

    /// The current value of an appearance setting, to restore after varying it.
    async fn appearance(&self, _device: &Device, _kind: &AppearanceKind) -> Result<Appearance> {
        Err(unsupported("reading appearance settings"))
    }

    /// The panel's own size and density, whatever is overridden.
    async fn physical_display(&self, _device: &Device) -> Result<PhysicalDisplay> {
        Err(unsupported("reading the display size"))
    }

    /// Changes an appearance setting; running apps get a configuration change.
    async fn set_appearance(&self, _device: &Device, _value: &Appearance) -> Result<()> {
        Err(unsupported("changing appearance settings"))
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

fn unsupported(operation: &str) -> Error {
    Error::Unsupported {
        operation: operation.to_owned(),
    }
}
