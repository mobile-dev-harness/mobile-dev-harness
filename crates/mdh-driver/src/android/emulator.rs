//! Android Virtual Devices: which can be started, and starting emulators the way the SDK does
//! (detached, waiting for `sys.boot_completed`), telling the new one apart from those already
//! connected.

use std::collections::HashSet;
use std::env;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use mdh_core::{Device, DeviceState, Error, Result};

use super::adb::Adb;

/// Cold boots take a minute or two on slower machines and in CI.
const BOOT_TIMEOUT: Duration = Duration::from_secs(240);
const POLL: Duration = Duration::from_secs(1);

/// Where AVD definitions live, as the emulator itself resolves it.
fn avd_home() -> Option<PathBuf> {
    if let Some(dir) = env::var_os("ANDROID_AVD_HOME") {
        return Some(dir.into());
    }
    if let Some(user) = env::var_os("ANDROID_USER_HOME") {
        return Some(PathBuf::from(user).join("avd"));
    }
    if let Some(sdk_home) = env::var_os("ANDROID_SDK_HOME") {
        return Some(PathBuf::from(sdk_home).join(".android/avd"));
    }
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(|h| PathBuf::from(h).join(".android/avd"))
}

/// The API level of an AVD from `<name>.ini` (`target=android-36`), else its `config.ini`
/// (`image.sysdir.1=system-images/android-36/…`).
pub(crate) fn avd_api(name: &str) -> Option<u32> {
    let home = avd_home()?;
    let ini = std::fs::read_to_string(home.join(format!("{name}.ini"))).ok()?;
    let value = |text: &str, key: &str| {
        text.lines().find_map(|l| {
            l.trim()
                .strip_prefix(key)
                .map(|v| v.trim().trim_start_matches('=').trim().to_owned())
        })
    };
    if let Some(api) = value(&ini, "target").as_deref().and_then(parse_api) {
        return Some(api);
    }
    let path = value(&ini, "path")?;
    let config = std::fs::read_to_string(Path::new(&path).join("config.ini")).ok()?;
    value(&config, "image.sysdir.1")?
        .split('/')
        .find_map(parse_api)
}

/// `android-36` → 36; preview codenames have no number.
pub(crate) fn parse_api(s: &str) -> Option<u32> {
    s.trim()
        .strip_prefix("android-")?
        .split(['.', '-'])
        .next()?
        .parse()
        .ok()
}

/// Starts `avd` and waits until it has booted. The emulator outlives mdh: it runs in its own
/// process group and isn't killed when the command ends.
pub(crate) async fn start(adb: &Adb, emulator: &Path, avd: &str, headless: bool) -> Result<Device> {
    let failed = |reason: String| Error::EmulatorFailed {
        avd: avd.to_owned(),
        reason,
    };
    let before: HashSet<String> = adb.devices().await?.into_iter().map(|d| d.id).collect();
    let log = env::temp_dir().join(format!("mdh-emulator-{avd}.log"));
    let out = File::create(&log)?;
    let mut command = Command::new(emulator);
    command.args(["-avd", avd, "-netdelay", "none", "-netspeed", "full"]);
    if headless {
        command.args(["-no-window", "-no-audio", "-no-boot-anim"]);
    }
    command
        .stdin(Stdio::null())
        .stdout(out.try_clone()?)
        .stderr(out);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|e| failed(e.to_string()))?;
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            let output = std::fs::read_to_string(&log).unwrap_or_default();
            // The emulator logs a lot; the line saying why is marked.
            let why = output
                .lines()
                .find(|l| l.contains("PANIC") || l.contains("ERROR") || l.contains("FATAL"))
                .or_else(|| output.lines().rev().find(|l| !l.trim().is_empty()))
                .unwrap_or("no output");
            // Messages without a newline run into the next log line.
            let why = why.split("INFO ").next().unwrap_or(why).trim();
            return Err(failed(format!("the emulator exited ({status}): {why}")));
        }
        // Only an emulator that wasn't there before can be the one just started.
        for d in adb.devices().await? {
            if before.contains(&d.id) || !d.is_emulator || d.state != DeviceState::Online {
                continue;
            }
            let (name, api) = identity(adb, &d.id).await;
            if name.as_deref().is_some_and(|n| n != avd) {
                continue;
            }
            if booted(adb, &d.id).await {
                return Ok(Device {
                    avd: Some(avd.to_owned()),
                    api,
                    ..d
                });
            }
        }
        if started.elapsed() > BOOT_TIMEOUT {
            let _ = child.kill();
            return Err(failed(format!(
                "not booted after {} s (log: {})",
                BOOT_TIMEOUT.as_secs(),
                log.display()
            )));
        }
        tokio::time::sleep(POLL).await;
    }
}

async fn booted(adb: &Adb, serial: &str) -> bool {
    adb.shell(serial, "getprop sys.boot_completed")
        .await
        .is_ok_and(|out| out.trim() == "1")
}

/// The AVD an emulator runs and the API level of any device. Older emulator images lack
/// `ro.boot.qemu.avd_name`; the emulator console still knows it.
pub(crate) async fn identity(adb: &Adb, serial: &str) -> (Option<String>, Option<u32>) {
    let out = adb
        .shell(
            serial,
            "getprop ro.boot.qemu.avd_name; getprop ro.build.version.sdk",
        )
        .await
        .unwrap_or_default();
    let mut lines = out.lines().map(str::trim);
    let name = lines.next().filter(|n| !n.is_empty()).map(str::to_owned);
    let api = lines.next().and_then(|a| a.parse().ok());
    if name.is_some() || !serial.starts_with("emulator-") {
        return (name, api);
    }
    let console = adb
        .on(serial, &["emu", "avd", "name"])
        .await
        .unwrap_or_default();
    let name = console
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && *l != "OK")
        .map(str::to_owned);
    (name, api)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_api_levels() {
        assert_eq!(parse_api("android-36"), Some(36));
        assert_eq!(parse_api("android-35-ext14"), Some(35));
        assert_eq!(parse_api("android-36.1"), Some(36));
        assert_eq!(parse_api("android-Baklava"), None);
        assert_eq!(parse_api("google_apis"), None);
    }
}
