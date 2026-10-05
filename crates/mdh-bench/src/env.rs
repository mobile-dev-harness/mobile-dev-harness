//! Where things are: the repository, the apps, the tools, the device.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::app::App;

/// The time zone every run starts in (bench/DESIGN.md, 4). Demo articles are published at 23:00 UTC:
/// a date formatted in UTC instead of local time shows only east of UTC+1.
pub const TIME_ZONE: &str = "America/Los_Angeles";

#[derive(Debug, Clone)]
pub struct Env {
    pub repo: PathBuf,
    pub sdk: PathBuf,
    /// The release build of mdh the agent (setup D) and the grader use.
    pub mdh: PathBuf,
    /// The Claude Code plugin setup D loads: a copy taken when the run started (see `main::run`).
    pub plugin: PathBuf,
    pub serial: String,
    /// The device the run started on; every run must find the same one.
    pub device: Identity,
    pub apps: BTreeMap<String, App>,
}

/// What makes runs on a device comparable: an agent that restarts the emulator with another AVD
/// changes the screen every later run (and the grader) sees.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Identity {
    pub avd: String,
    pub api: String,
    /// `wm size` and `wm density` without overrides: the display as reset leaves it.
    pub size: String,
    pub density: String,
}

impl Identity {
    /// The output of `getprop` (AVD name, API level), `wm size` and `wm density`, in that order.
    fn parse(out: &str) -> Option<Identity> {
        let lines: Vec<&str> = out.lines().map(str::trim).collect();
        let physical = |key: &str| {
            lines
                .iter()
                .find_map(|l| l.strip_prefix(key))
                .map(|v| v.trim().to_owned())
        };
        let api = lines.get(1).filter(|a| !a.is_empty())?;
        Some(Identity {
            avd: (*lines.first()?).to_owned(),
            api: (*api).to_owned(),
            size: physical("Physical size:")?,
            density: physical("Physical density:")?,
        })
    }
}

impl std::fmt::Display for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} (API {}, {}, {} dpi)",
            self.avd, self.api, self.size, self.density
        )
    }
}

impl Env {
    pub fn detect(repo: &Path) -> Result<Env, String> {
        let repo = repo
            .canonicalize()
            .map_err(|e| format!("{}: {e}", repo.display()))?;
        let sdk = std::env::var_os("ANDROID_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::fs::read_to_string(repo.join("examples/android-sample/local.properties"))
                    .ok()?
                    .lines()
                    .find_map(|l| l.strip_prefix("sdk.dir=").map(PathBuf::from))
            })
            .ok_or("no Android SDK: set ANDROID_HOME")?;
        let mdh = repo.join("target/release/mdh");
        if !mdh.is_file() {
            return Err("build mdh first: cargo build --release --bin mdh".into());
        }
        let plugin = repo.join("integrations/claude-code");
        let apps = crate::app::load_all(&repo)?;
        let env = Env {
            repo,
            sdk,
            mdh,
            plugin,
            serial: String::new(),
            device: Identity::default(),
            apps,
        };
        let out = env.adb(&["devices"])?;
        let online: Vec<&str> = out
            .lines()
            .skip(1)
            .filter(|l| l.ends_with("\tdevice"))
            .filter_map(|l| l.split('\t').next())
            .collect();
        match online.as_slice() {
            [one] => {
                let env = Env {
                    serial: (*one).to_owned(),
                    ..env
                };
                let device = env.identity()?;
                Ok(Env { device, ..env })
            }
            [] => Err("no emulator online".into()),
            _ => Err(format!(
                "several devices online ({}); keep one",
                online.join(", ")
            )),
        }
    }

    pub fn app(&self, name: &str) -> &App {
        &self.apps[name]
    }

    pub fn adb_path(&self) -> PathBuf {
        self.sdk.join("platform-tools/adb")
    }

    pub fn adb(&self, args: &[&str]) -> Result<String, String> {
        let mut cmd = Command::new(self.adb_path());
        if !self.serial.is_empty() {
            cmd.args(["-s", &self.serial]);
        }
        let out = cmd.args(args).output().map_err(|e| format!("adb: {e}"))?;
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// The device online now, as reset leaves it.
    pub fn identity(&self) -> Result<Identity, String> {
        let out = self.adb(&[
            "shell",
            "getprop ro.boot.qemu.avd_name; getprop ro.build.version.sdk; wm size; wm density",
        ])?;
        Identity::parse(&out).ok_or_else(|| format!("{} doesn't answer", self.serial))
    }

    /// Fails unless the device is the one the run started on.
    pub fn check_device(&self) -> Result<(), String> {
        let now = self.identity()?;
        if now == self.device {
            Ok(())
        } else {
            Err(format!("the device changed: {} became {now}", self.device))
        }
    }

    /// The device as every run finds it: no app of the benchmark installed, settings at their
    /// defaults, home screen.
    pub fn reset_device(&self) {
        for app in self.apps.values() {
            let _ = self.adb(&[
                "shell",
                &format!("am force-stop {0}; pm uninstall {0}", app.package),
            ]);
        }
        let _ = self.adb(&["shell", &format!("cmd alarm set-timezone {TIME_ZONE}")]);
        for cmd in [
            "settings put global window_animation_scale 1",
            "settings put global transition_animation_scale 1",
            "settings put global animator_duration_scale 1",
            "settings put system accelerometer_rotation 1",
            "settings put system user_rotation 0",
            "settings delete system font_scale",
            "cmd uimode night no",
            "wm size reset",
            "wm density reset",
            // Agents disable the keyboard to type with adb.
            "ime reset",
            "input keyevent HOME",
        ] {
            let _ = self.adb(&["shell", cmd]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_physical_display_not_an_override() {
        let out = "Pixel_9_Pro_XL\n36\nPhysical size: 1344x2992\nOverride size: 1080x2400\n\
                   Physical density: 480\n";
        let id = Identity::parse(out).unwrap();
        assert_eq!(
            id.to_string(),
            "Pixel_9_Pro_XL (API 36, 1344x2992, 480 dpi)"
        );
        assert_eq!(Identity::parse("error: device offline\n"), None);
    }
}
