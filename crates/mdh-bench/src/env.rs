//! Where things are: the repository, the sample app, the tools, the device.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct Env {
    pub repo: PathBuf,
    pub sdk: PathBuf,
    /// The release build of mdh the agent (setup D) and the grader use.
    pub mdh: PathBuf,
    pub serial: String,
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
        let env = Env {
            repo,
            sdk,
            mdh,
            serial: String::new(),
        };
        let out = env.adb(&["devices"])?;
        let online: Vec<&str> = out
            .lines()
            .skip(1)
            .filter(|l| l.ends_with("\tdevice"))
            .filter_map(|l| l.split('\t').next())
            .collect();
        match online.as_slice() {
            [one] => Ok(Env {
                serial: (*one).to_owned(),
                ..env
            }),
            [] => Err("no emulator online".into()),
            _ => Err(format!(
                "several devices online ({}); keep one",
                online.join(", ")
            )),
        }
    }

    pub fn sample(&self) -> PathBuf {
        self.repo.join("examples/android-sample")
    }

    pub fn plugin(&self) -> PathBuf {
        self.repo.join("integrations/claude-code")
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

    /// The device as every run finds it: the app gone, settings at their defaults, home screen.
    pub fn reset_device(&self) {
        for cmd in [
            "am force-stop dev.mdh.sample",
            "pm uninstall dev.mdh.sample",
            "settings put global window_animation_scale 1",
            "settings put global transition_animation_scale 1",
            "settings put global animator_duration_scale 1",
            "settings put system accelerometer_rotation 1",
            "settings put system user_rotation 0",
            "settings delete system font_scale",
            "cmd uimode night no",
            "wm size reset",
            "wm density reset",
            "input keyevent HOME",
        ] {
            let _ = self.adb(&["shell", cmd]);
        }
    }
}
