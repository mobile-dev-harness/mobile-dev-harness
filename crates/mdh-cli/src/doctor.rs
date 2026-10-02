//! `mdh doctor`: report whether the local toolchain can build and drive Android apps.

use mdh_core::Error;
use mdh_driver::android::{Adb, AndroidSdk};
use serde::Serialize;
use tokio::process::Command;

use crate::output::Human;

#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Status {
    Ok,
    Warn,
    Fail,
}

#[derive(Serialize)]
struct Check {
    name: &'static str,
    status: Status,
    detail: String,
}

impl Check {
    fn new(name: &'static str, status: Status, detail: impl Into<String>) -> Self {
        Self {
            name,
            status,
            detail: detail.into(),
        }
    }
}

#[derive(Serialize)]
#[serde(transparent)]
pub struct Checks(Vec<Check>);

impl Human for Checks {
    fn human(&self) -> String {
        self.0
            .iter()
            .map(|c| {
                let mark = match c.status {
                    Status::Ok => "✓",
                    Status::Warn => "!",
                    Status::Fail => "✗",
                };
                format!("{mark} {:<12} {}", c.name, c.detail)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Runs every check. Failing checks turn into `EnvironmentNotReady`; the checks are returned either way.
pub async fn run() -> (Checks, Option<Error>) {
    let checks = collect().await;
    let failed: Vec<String> = checks
        .iter()
        .filter(|c| c.status == Status::Fail)
        .map(|c| c.name.to_owned())
        .collect();
    let error = (!failed.is_empty()).then_some(Error::EnvironmentNotReady { failed });
    (Checks(checks), error)
}

async fn collect() -> Vec<Check> {
    let mut checks = Vec::new();

    let sdk = match AndroidSdk::locate() {
        Ok(sdk) => sdk,
        Err(e) => {
            checks.push(Check::new("adb", Status::Fail, with_hint(&e)));
            checks.push(java().await);
            return checks;
        }
    };

    checks.push(match &sdk.root {
        Some(root) => Check::new("android-sdk", Status::Ok, root.display().to_string()),
        None => Check::new(
            "android-sdk",
            Status::Warn,
            "SDK root not found; using tools from PATH (set ANDROID_HOME)",
        ),
    });

    let adb = Adb::new(sdk.adb.clone());
    checks.push(match adb.version().await {
        Ok(v) => Check::new("adb", Status::Ok, format!("{v} ({})", adb.path().display())),
        Err(e) => Check::new("adb", Status::Fail, with_hint(&e)),
    });

    checks.push(match &sdk.emulator {
        None => Check::new(
            "emulator",
            Status::Warn,
            "not installed; only physical devices can be used",
        ),
        Some(_) => match sdk.avds().await {
            Ok(avds) if avds.is_empty() => Check::new(
                "emulator",
                Status::Warn,
                "no AVDs found; create one in Android Studio or with avdmanager",
            ),
            Ok(avds) => Check::new("emulator", Status::Ok, format!("AVDs: {}", avds.join(", "))),
            Err(e) => Check::new("emulator", Status::Warn, with_hint(&e)),
        },
    });

    checks.push(java().await);

    checks.push(match adb.devices().await {
        Ok(d) if d.is_empty() => Check::new("devices", Status::Warn, "none connected"),
        Ok(d) => Check::new("devices", Status::Ok, format!("{} connected", d.len())),
        Err(e) => Check::new("devices", Status::Fail, with_hint(&e)),
    });

    checks
}

/// Gradle needs a JDK. `java -version` prints to stderr.
async fn java() -> Check {
    match Command::new("java").arg("-version").output().await {
        Ok(out) if out.status.success() => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let version = stderr.lines().next().unwrap_or_default().trim().to_owned();
            Check::new("java", Status::Ok, version)
        }
        // e.g. the macOS `/usr/bin/java` stub when no JDK is installed.
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let reason = stderr
                .lines()
                .next()
                .unwrap_or_default()
                .trim()
                .trim_end_matches('.');
            Check::new(
                "java",
                Status::Fail,
                format!("{reason}; Gradle builds need a JDK (17+)"),
            )
        }
        Err(_) => Check::new(
            "java",
            Status::Fail,
            "not found; Gradle builds need a JDK (17+)",
        ),
    }
}

fn with_hint(e: &Error) -> String {
    format!("{e}; {}", e.hint())
}
