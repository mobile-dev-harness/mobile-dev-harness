//! Android backend built on the platform-tools CLIs and the on-device helper.

mod adb;
mod am;
mod display;
mod emulator;
mod helper;
mod install;
mod logcat;
mod perf;
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
use mdh_core::{
    Appearance, AppearanceKind, Avd, Device, Error, FrameStats, Input, LaunchInfo, LogEntry,
    MemoryStats, PhysicalDisplay, Platform, Result,
};

use crate::Driver;

/// Upper bound on log lines fetched per read, so a long pause between calls can't flood memory.
const MAX_LOG_LINES: usize = 5000;

/// Where traces are written: the one directory perfetto may write to on user builds.
const TRACE_FILE: &str = "/data/misc/perfetto-traces/mdh.pftrace";
const TRACE_CONFIG: &str = "/data/local/tmp/mdh-perfetto.cfg";
/// The PID of the tracing perfetto, so stopping it leaves other traces alone.
const TRACE_PID: &str = "/data/local/tmp/mdh-perfetto.pid";

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
            let id = emulator::identity(&self.adb, &d.id).await;
            d.avd = id.avd.filter(|_| d.is_emulator);
            d.api = id.api;
            d.manufacturer = id.manufacturer;
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

    async fn frame_stats(&self, device: &Device, package: &str, reset: bool) -> Result<FrameStats> {
        let command = format!(
            "dumpsys gfxinfo {}{}",
            shell_quote(package),
            if reset { " reset" } else { "" }
        );
        let out = self.adb.shell(&device.id, &command).await?;
        // A reset prints the statistics it discards, which may be nothing for a new process.
        Ok(perf::parse_gfxinfo(&out).unwrap_or_default())
    }

    async fn memory(&self, device: &Device, package: &str) -> Result<MemoryStats> {
        let out = self
            .adb
            .shell(
                &device.id,
                &format!("dumpsys meminfo {}", shell_quote(package)),
            )
            .await?;
        perf::parse_meminfo(&out).ok_or_else(|| Error::AppNotFound {
            package: package.to_owned(),
        })
    }

    async fn cpu_time_ms(&self, device: &Device, pid: u32) -> Result<u64> {
        let out = self
            .adb
            .shell(&device.id, &format!("cat /proc/{pid}/stat"))
            .await?;
        // Android's clock tick is 10 ms (CLK_TCK 100).
        perf::parse_proc_stat_ticks(&out)
            .map(|t| t * 10)
            .ok_or(Error::Parse {
                tool: "/proc/stat".into(),
                detail: out,
            })
    }

    async fn debuggable(&self, device: &Device, package: &str) -> Result<bool> {
        let out = self
            .adb
            .shell_stdout(
                &device.id,
                &format!("dumpsys package {}", shell_quote(package)),
            )
            .await?;
        Ok(out
            .lines()
            .any(|l| l.contains("flags=[") && l.contains("DEBUGGABLE")))
    }

    async fn start_trace(&self, device: &Device, config: &str) -> Result<()> {
        // The config goes through a file: perfetto doesn't read a heredoc on stdin when it
        // detaches, but does read a pipe. `--background-wait` returns once every data source
        // records (so the first events of what follows aren't lost); before Android 12 there's
        // only `--background`, and a pause instead. Either prints the PID.
        let command = format!(
            "rm -f {TRACE_FILE}; cat > {TRACE_CONFIG} <<'MDH_EOF'\n{config}\nMDH_EOF\n\
             P=$(cat {TRACE_CONFIG} | perfetto --background-wait --txt -c - -o {TRACE_FILE} 2>/dev/null) \
             || {{ P=$(cat {TRACE_CONFIG} | perfetto --background --txt -c - -o {TRACE_FILE}) && sleep 1; }}; \
             echo \"$P\" > {TRACE_PID}; echo \"$P\""
        );
        let pid = self.adb.shell_stdout(&device.id, &command).await?;
        if pid.trim().parse::<u32>().is_err() {
            return Err(Error::CommandFailed {
                command: "perfetto --background".into(),
                code: None,
                stderr: format!("tracing didn't start: {}", pid.trim()),
            });
        }
        Ok(())
    }

    async fn stop_trace(&self, device: &Device) -> Result<Vec<u8>> {
        // SIGTERM makes perfetto flush and write the trace. The file fills in shortly after the
        // process is gone, so wait for both: the process to exit, the file to stop growing.
        let _ = self
            .adb
            .shell(
                &device.id,
                &format!("kill -TERM $(cat {TRACE_PID}) 2>/dev/null"),
            )
            .await;
        for _ in 0..40 {
            let running = self
                .adb
                .shell_stdout(
                    &device.id,
                    &format!("kill -0 $(cat {TRACE_PID}) 2>/dev/null && echo running"),
                )
                .await
                .unwrap_or_default();
            if running.trim().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        let mut last = 0u64;
        for _ in 0..40 {
            let size = self
                .adb
                .shell_stdout(&device.id, &format!("stat -c %s {TRACE_FILE}"))
                .await
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok())
                .unwrap_or(0);
            if size > 0 && size == last {
                break;
            }
            last = size;
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        self.adb
            .exec_out_bytes(&device.id, &["cat", TRACE_FILE])
            .await
    }

    async fn appearance(&self, device: &Device, kind: &AppearanceKind) -> Result<Appearance> {
        let shell = |cmd: String| async move { self.adb.shell(&device.id, &cmd).await };
        Ok(match kind {
            AppearanceKind::FontScale => {
                let v = shell("settings get system font_scale".into()).await?;
                let v = v.trim();
                Appearance::FontScale((v != "null" && !v.is_empty()).then(|| v.to_owned()))
            }
            AppearanceKind::NightMode => {
                let out = shell("cmd uimode night".into()).await?;
                let v = out
                    .trim()
                    .rsplit(':')
                    .next()
                    .unwrap_or("no")
                    .trim()
                    .to_owned();
                Appearance::NightMode(v)
            }
            AppearanceKind::AppLocales { package } => {
                // `Locales for dev.app for user 0 are [ar,en]`
                let out = shell(format!(
                    "cmd locale get-app-locales {}",
                    shell_quote(package)
                ))
                .await?;
                let locales = out
                    .split_once('[')
                    .and_then(|(_, r)| r.split_once(']'))
                    .map(|(l, _)| l.trim().to_owned())
                    .unwrap_or_default();
                Appearance::AppLocales {
                    package: package.clone(),
                    locales,
                }
            }
            AppearanceKind::Rotation => {
                let out = shell(
                    "settings get system accelerometer_rotation; settings get system user_rotation"
                        .into(),
                )
                .await?;
                let mut lines = out.lines().map(str::trim);
                let auto = lines.next() != Some("0");
                let user = lines.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                Appearance::Rotation { auto, user }
            }
            AppearanceKind::Display => {
                let out = shell("wm size; wm density".into()).await?;
                let d = display::parse(&out);
                Appearance::Display {
                    size: d.override_size,
                    density: d.override_density,
                }
            }
        })
    }

    async fn physical_display(&self, device: &Device) -> Result<PhysicalDisplay> {
        let out = self.adb.shell(&device.id, "wm size; wm density").await?;
        display::parse(&out).physical.ok_or_else(|| Error::Parse {
            tool: "wm size".into(),
            detail: out.trim().to_owned(),
        })
    }

    async fn set_appearance(&self, device: &Device, value: &Appearance) -> Result<()> {
        let command = match value {
            Appearance::FontScale(Some(v)) => {
                format!("settings put system font_scale {}", shell_quote(v))
            }
            Appearance::FontScale(None) => "settings delete system font_scale".into(),
            Appearance::NightMode(v) => format!("cmd uimode night {}", shell_quote(v)),
            Appearance::AppLocales { package, locales } if locales.is_empty() => {
                format!("cmd locale set-app-locales {}", shell_quote(package))
            }
            Appearance::AppLocales { package, locales } => format!(
                "cmd locale set-app-locales {} --locales {}",
                shell_quote(package),
                shell_quote(locales)
            ),
            Appearance::Rotation { auto, user } => format!(
                "settings put system user_rotation {}; settings put system accelerometer_rotation {}",
                user % 4,
                u8::from(*auto)
            ),
            Appearance::Display { size, density } => format!(
                "wm size {}; wm density {}",
                size.map_or("reset".into(), |(w, h)| format!("{w}x{h}")),
                density.map_or("reset".into(), |d| d.to_string())
            ),
        };
        self.adb.shell(&device.id, &command).await.map(drop)
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
