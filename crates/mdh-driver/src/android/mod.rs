//! Android backend built on the platform-tools CLIs and the on-device helper.

mod adb;
mod am;
mod emulator;
mod helper;
mod install;
mod logcat;
mod sdk;
mod uiautomator;

pub use adb::{Adb, shell_quote};
pub use helper::{HELPER_VERSION_CODE, Helper};
pub use logcat::parse_logcat;
pub use sdk::AndroidSdk;
pub use uiautomator::parse_hierarchy;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use mdh_core::ui::{RawTree, TreeSource};
use mdh_core::{Avd, Device, Error, Input, LaunchInfo, LogEntry, Platform, Result};

use crate::Driver;

/// Upper bound on log lines fetched per read, so a long pause between calls can't flood memory.
const MAX_LOG_LINES: usize = 5000;

/// Global settings that scale window, transition and animator animations.
const ANIMATION_SCALES: [&str; 3] = [
    "window_animation_scale",
    "transition_animation_scale",
    "animator_duration_scale",
];

pub struct AndroidDriver {
    adb: Adb,
    sdk_root: Option<PathBuf>,
    emulator: Option<PathBuf>,
    /// Helpers already verified in this process, by device serial.
    helpers: Mutex<HashMap<String, Arc<Helper>>>,
}

impl AndroidDriver {
    pub fn new(sdk: &AndroidSdk) -> Self {
        Self {
            adb: Adb::new(sdk.adb.clone()),
            sdk_root: sdk.root.clone(),
            emulator: sdk.emulator.clone(),
            helpers: Mutex::default(),
        }
    }

    pub fn adb(&self) -> &Adb {
        &self.adb
    }

    async fn helper(&self, device: &Device) -> Result<Arc<Helper>> {
        if let Some(helper) = self.helpers.lock().expect("not poisoned").get(&device.id) {
            return Ok(helper.clone());
        }
        let helper = Arc::new(Helper::ensure(&self.adb, &device.id).await?);
        self.helpers
            .lock()
            .expect("not poisoned")
            .insert(device.id.clone(), helper.clone());
        Ok(helper)
    }

    /// Runs `f` against the helper; on `HelperUnavailable` forgets it so the next call re-checks.
    async fn with_helper<T, F, Fut>(&self, device: &Device, f: F) -> Result<T>
    where
        F: FnOnce(Arc<Helper>) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let result = f(self.helper(device).await?).await;
        if let Err(Error::HelperUnavailable { .. }) = result {
            self.helpers
                .lock()
                .expect("not poisoned")
                .remove(&device.id);
        }
        result
    }

    /// `uiautomator dump`: no installation needed, but ~2 s per call and it can't run while
    /// the helper holds the device's UiAutomation connection.
    async fn dump_with_uiautomator(&self, device: &Device) -> Result<RawTree> {
        let xml = self
            .adb
            .on_with_timeout(
                &device.id,
                &["exec-out", "uiautomator", "dump", "/dev/tty"],
                Duration::from_secs(30),
            )
            .await?;
        Ok(RawTree {
            roots: parse_hierarchy(&xml)?,
            source: TreeSource::Uiautomator,
            windows: Vec::new(),
        })
    }

    /// `adb shell input`, ~120 ms per call because it starts a Java process each time.
    async fn input_with_adb(&self, device: &Device, input: &Input) -> Result<()> {
        let command = match input {
            Input::Tap { x, y } => format!("input tap {x} {y}"),
            // `input swipe` can't hold at the end; scrolls may fling a little further.
            Input::Swipe {
                from,
                to,
                duration_ms,
                ..
            } => format!(
                "input swipe {} {} {} {} {duration_ms}",
                from.0, from.1, to.0, to.1
            ),
            Input::Key { name } => {
                format!("input keyevent {}", shell_quote(&format!("KEYCODE_{name}")))
            }
            Input::SetText { .. } => {
                return Err(Error::HelperUnavailable {
                    reason: "text input needs the on-device helper".into(),
                });
            }
        };
        self.adb.shell(&device.id, &command).await.map(drop)
    }
}

#[async_trait]
impl Driver for AndroidDriver {
    fn platform(&self) -> Platform {
        Platform::Android
    }

    async fn devices(&self) -> Result<Vec<Device>> {
        let mut devices = self.adb.devices().await?;
        for d in devices
            .iter_mut()
            .filter(|d| d.state == mdh_core::DeviceState::Online)
        {
            let (avd, api) = emulator::identity(&self.adb, &d.id).await;
            d.avd = avd.filter(|_| d.is_emulator);
            d.api = api;
        }
        Ok(devices)
    }

    async fn avds(&self) -> Result<Vec<Avd>> {
        let names = AndroidSdk {
            root: self.sdk_root.clone(),
            adb: PathBuf::new(),
            emulator: self.emulator.clone(),
        }
        .avds()
        .await?;
        let running = self.devices().await?;
        Ok(names
            .into_iter()
            .map(|name| Avd {
                api: emulator::avd_api(&name),
                running: running
                    .iter()
                    .find(|d| d.avd.as_deref() == Some(name.as_str()))
                    .map(|d| d.id.clone()),
                name,
            })
            .collect())
    }

    async fn start_emulator(&self, avd: &str, headless: bool) -> Result<Device> {
        if let Some(serial) = self
            .avds()
            .await?
            .into_iter()
            .find(|a| a.name == avd)
            .and_then(|a| a.running)
        {
            return self
                .devices()
                .await?
                .into_iter()
                .find(|d| d.id == serial)
                .ok_or(Error::DeviceNotFound { id: serial });
        }
        let path = self.emulator.as_deref().ok_or_else(|| Error::EmulatorFailed {
            avd: avd.to_owned(),
            reason: "the Android Emulator isn't installed (Android Studio → SDK Manager → SDK Tools)".into(),
        })?;
        emulator::start(&self.adb, path, avd, headless).await
    }

    async fn stop_emulator(&self, device: &Device) -> Result<()> {
        if !device.is_emulator {
            return Err(Error::InvalidTarget {
                target: device.id.clone(),
                reason: "not an emulator; physical devices aren't shut down".into(),
            });
        }
        self.helpers
            .lock()
            .expect("not poisoned")
            .remove(&device.id);
        self.adb.on(&device.id, &["emu", "kill"]).await?;
        // `emu kill` returns at once; the emulator lingers in `adb devices` while it shuts down.
        for _ in 0..40 {
            let gone = self.adb.devices().await?.iter().all(|d| d.id != device.id);
            if gone {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Ok(())
    }

    async fn ui_tree(&self, device: &Device) -> Result<RawTree> {
        let result = self
            .with_helper(device, |helper| async move { helper.tree().await })
            .await;
        match result {
            Ok((roots, windows)) => Ok(RawTree {
                roots,
                source: TreeSource::Helper,
                windows,
            }),
            // Nothing is on screen for a moment while an app dies or between windows; an empty
            // tree lets settling and crash reporting carry on.
            Err(Error::HelperCommand { message, .. })
                if message.contains("no window with content") =>
            {
                Ok(RawTree {
                    roots: Vec::new(),
                    source: TreeSource::Helper,
                    windows: Vec::new(),
                })
            }
            Err(Error::HelperUnavailable { .. }) => self.dump_with_uiautomator(device).await,
            Err(e) => Err(e),
        }
    }

    async fn foreground_activity(&self, device: &Device) -> Result<Option<String>> {
        // `grep -m1` closes the pipe early; dumpsys complains on stderr, which is ignored.
        let out = self
            .adb
            .shell_stdout(&device.id, "dumpsys window displays | grep -m1 mFocusedApp")
            .await?;
        Ok(am::parse_focused_app(&out))
    }

    async fn input(&self, device: &Device, input: &Input) -> Result<()> {
        let result = self
            .with_helper(device, |helper| async move { helper.input(input).await })
            .await;
        match result {
            Err(Error::HelperUnavailable { .. }) => self.input_with_adb(device, input).await,
            other => other,
        }
    }

    async fn screenshot(&self, device: &Device) -> Result<Vec<u8>> {
        self.adb
            .exec_out_bytes(&device.id, &["screencap", "-p"])
            .await
    }

    async fn install(&self, device: &Device, apk: &Path, grant_permissions: bool) -> Result<()> {
        let apk = apk.to_string_lossy();
        // -d: debug builds may go back in version (e.g. after switching branches).
        let mut args = vec!["install", "-r", "-t", "-d"];
        if grant_permissions {
            args.push("-g");
        }
        args.push(&apk);
        match self
            .adb
            .on_with_timeout(&device.id, &args, Duration::from_secs(300))
            .await
        {
            Err(Error::CommandFailed { stderr, .. })
                if install::parse_failure(&stderr).is_some() =>
            {
                let (reason, detail) = install::parse_failure(&stderr).expect("checked");
                Err(Error::InstallFailed { reason, detail })
            }
            other => other.map(drop),
        }
    }

    async fn uninstall(&self, device: &Device, package: &str) -> Result<()> {
        // Fails when the package isn't installed, which is fine here.
        let _ = self.adb.on(&device.id, &["uninstall", package]).await;
        Ok(())
    }

    async fn abis(&self, device: &Device) -> Result<Vec<String>> {
        let out = self
            .adb
            .shell(&device.id, "getprop ro.product.cpu.abilist")
            .await?;
        Ok(out
            .trim()
            .split(',')
            .filter(|a| !a.is_empty())
            .map(str::to_owned)
            .collect())
    }

    fn sdk_root(&self) -> Option<PathBuf> {
        self.sdk_root.clone()
    }

    async fn launch(&self, device: &Device, app: &str) -> Result<LaunchInfo> {
        let component = if app.contains('/') {
            app.to_owned()
        } else {
            let out = self
                .adb
                .shell(
                    &device.id,
                    &format!(
                        "cmd package resolve-activity --brief -c android.intent.category.LAUNCHER {}",
                        shell_quote(app)
                    ),
                )
                .await?;
            am::parse_resolved_activity(&out).ok_or_else(|| Error::AppNotFound {
                package: app.to_owned(),
            })?
        };
        // Failures are reported on stdout with a non-zero exit status.
        let out = self
            .adb
            .shell_stdout(
                &device.id,
                &format!("am start -W -n {}", shell_quote(&component)),
            )
            .await?;
        am::parse_am_start(&component, &out)
    }

    async fn open_uri(
        &self,
        device: &Device,
        uri: &str,
        package: Option<&str>,
    ) -> Result<LaunchInfo> {
        let mut command = format!(
            "am start -W -a android.intent.action.VIEW -d {}",
            shell_quote(uri)
        );
        if let Some(p) = package {
            command.push(' ');
            command.push_str(&shell_quote(p));
        }
        let out = self.adb.shell_stdout(&device.id, &command).await?;
        am::parse_am_start(uri, &out)
    }

    async fn clear_data(&self, device: &Device, package: &str) -> Result<()> {
        let out = self
            .adb
            .shell_stdout(&device.id, &format!("pm clear {}", shell_quote(package)))
            .await?;
        if out.trim() == "Success" {
            Ok(())
        } else {
            Err(Error::AppNotFound {
                package: package.to_owned(),
            })
        }
    }

    async fn density(&self, device: &Device) -> Result<u32> {
        let out = self.adb.shell(&device.id, "wm density").await?;
        am::parse_density(&out).ok_or(Error::Parse {
            tool: "wm density".into(),
            detail: out,
        })
    }

    async fn animation_scales(&self, device: &Device) -> Result<Vec<(String, Option<String>)>> {
        let command: Vec<String> = ANIMATION_SCALES
            .iter()
            .map(|s| format!("settings get global {s}"))
            .collect();
        let out = self.adb.shell(&device.id, &command.join("; ")).await?;
        let values: Vec<&str> = out.lines().map(str::trim).collect();
        if values.len() != ANIMATION_SCALES.len() {
            return Err(Error::Parse {
                tool: "settings".into(),
                detail: out,
            });
        }
        Ok(ANIMATION_SCALES
            .iter()
            .zip(values)
            .map(|(name, v)| ((*name).to_owned(), (v != "null").then(|| v.to_owned())))
            .collect())
    }

    async fn set_animation_scales(
        &self,
        device: &Device,
        scales: &[(String, Option<String>)],
    ) -> Result<()> {
        let command: Vec<String> = scales
            .iter()
            .map(|(name, value)| match value {
                Some(v) => format!(
                    "settings put global {} {}",
                    shell_quote(name),
                    shell_quote(v)
                ),
                None => format!("settings delete global {}", shell_quote(name)),
            })
            .collect();
        self.adb
            .shell(&device.id, &command.join("; "))
            .await
            .map(drop)
    }

    async fn set_permission(
        &self,
        device: &Device,
        package: &str,
        permission: &str,
        granted: bool,
    ) -> Result<()> {
        let verb = if granted { "grant" } else { "revoke" };
        self.adb
            .shell(
                &device.id,
                &format!(
                    "pm {verb} {} {}",
                    shell_quote(package),
                    shell_quote(permission)
                ),
            )
            .await
            .map(drop)
    }

    async fn installed_path(&self, device: &Device, package: &str) -> Result<Option<String>> {
        // `pm path` prints `package:/data/app/~~…==/pkg-…==/base.apk` (plus split APKs) and exits
        // with 1 when the package is unknown.
        let out = self
            .adb
            .shell_stdout(&device.id, &format!("pm path {}", shell_quote(package)))
            .await?;
        Ok(out
            .lines()
            .find_map(|l| l.strip_prefix("package:"))
            .map(|p| p.trim().to_owned()))
    }

    async fn clock_ms(&self, device: &Device) -> Result<u64> {
        let out = self.adb.shell(&device.id, "date +%s.%N").await?;
        logcat::parse_device_time(&out).ok_or(Error::Parse {
            tool: "date".into(),
            detail: out,
        })
    }

    async fn logs(&self, device: &Device, since_ms: u64) -> Result<Vec<LogEntry>> {
        // `-T` takes `seconds.millis` on the device clock and is inclusive, hence the filter.
        let command = format!(
            "logcat -d -v epoch -v uid -b main,system,crash -T {}.{:03} | tail -n {MAX_LOG_LINES}",
            since_ms / 1000,
            since_ms % 1000
        );
        let out = self.adb.shell_stdout(&device.id, &command).await?;
        let mut entries = logcat::parse_logcat(&out);
        entries.retain(|e| e.time_ms > since_ms);
        Ok(entries)
    }

    async fn pids(&self, device: &Device, packages: &[String]) -> Result<Vec<u32>> {
        if packages.is_empty() {
            return Ok(Vec::new());
        }
        let quoted: Vec<String> = packages.iter().map(|p| shell_quote(p)).collect();
        // `pidof` exits with 1 when nothing runs, which is not an error here.
        let out = self
            .adb
            .shell_stdout(&device.id, &format!("pidof {}", quoted.join(" ")))
            .await?;
        Ok(logcat::parse_pids(&out))
    }

    async fn release(&self, device: &Device) -> Result<()> {
        self.helpers
            .lock()
            .expect("not poisoned")
            .remove(&device.id);
        helper::stop(&self.adb, &device.id).await
    }

    async fn wait_idle(&self, device: &Device, quiet: Duration, timeout: Duration) -> Result<bool> {
        let result = self
            .with_helper(device, |helper| async move {
                helper.wait_idle(quiet, timeout).await
            })
            .await;
        match result {
            Err(Error::HelperUnavailable { .. }) => Ok(false),
            other => other,
        }
    }

    async fn stop(&self, device: &Device, package: &str) -> Result<()> {
        self.adb
            .shell(
                &device.id,
                &format!("am force-stop {}", shell_quote(package)),
            )
            .await
            .map(drop)
    }
}
