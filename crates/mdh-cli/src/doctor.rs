//! `mdh doctor`: report whether the local toolchain can build and drive Android apps.

use std::process::ExitCode;

use mdh_driver::android::{Adb, AndroidSdk};
use serde::Serialize;
use tokio::process::Command;

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

pub async fn run(json: bool) -> anyhow::Result<ExitCode> {
    let checks = collect().await;

    if json {
        println!("{}", serde_json::to_string_pretty(&checks)?);
    } else {
        for c in &checks {
            let mark = match c.status {
                Status::Ok => "✓",
                Status::Warn => "!",
                Status::Fail => "✗",
            };
            println!("{mark} {:<12} {}", c.name, c.detail);
        }
    }

    let failed = checks.iter().any(|c| c.status == Status::Fail);
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

async fn collect() -> Vec<Check> {
    let mut checks = Vec::new();

    let sdk = match AndroidSdk::locate() {
        Ok(sdk) => sdk,
        Err(e) => {
            checks.push(Check::new("adb", Status::Fail, e.to_string()));
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
        Err(e) => Check::new("adb", Status::Fail, e.to_string()),
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
            Err(e) => Check::new("emulator", Status::Warn, e.to_string()),
        },
    });

    checks.push(java().await);

    checks.push(match adb.devices().await {
        Ok(d) if d.is_empty() => Check::new("devices", Status::Warn, "none connected"),
        Ok(d) => Check::new("devices", Status::Ok, format!("{} connected", d.len())),
        Err(e) => Check::new("devices", Status::Fail, e.to_string()),
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
        Ok(out) => Check::new(
            "java",
            Status::Fail,
            String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        ),
        Err(_) => Check::new(
            "java",
            Status::Fail,
            "not found; Gradle builds need a JDK (17+)",
        ),
    }
}
