//! Host side of the on-device helper (`android-helper/`): installs, starts and talks to it.

use std::time::{Duration, Instant};

use mdh_core::ui::{RawNode, WindowInfo};
use mdh_core::{Error, Input, Result};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use super::Adb;

/// Must match `versionCode` in android-helper/build.gradle.kts and `Commands.VERSION_CODE`.
pub const HELPER_VERSION_CODE: u64 = 5;

const PACKAGE: &str = "dev.mdh.helper";
const INSTRUMENTATION: &str = "dev.mdh.helper/.HelperInstrumentation";
const SOCKET: &str = "localabstract:mdh-helper";
/// Built by scripts/build-helper.sh.
static APK: &[u8] = include_bytes!("../../assets/mdh-helper.apk");

const START_TIMEOUT: Duration = Duration::from_secs(8);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// A connection to a running helper, reached through an `adb forward` to its abstract socket.
pub struct Helper {
    port: u16,
}

impl Helper {
    /// Connects to the helper on `serial`, installing, replacing or starting it as needed. Only
    /// the embedded version will do, so a newer one (another mdh's) is replaced like an older one.
    pub async fn ensure(adb: &Adb, serial: &str) -> Result<Helper> {
        let helper = Helper {
            port: forward(adb, serial).await?,
        };
        match helper.version().await {
            Ok(HELPER_VERSION_CODE) => return Ok(helper),
            Ok(_other) => {
                adb.shell(serial, &format!("am force-stop {PACKAGE}"))
                    .await?;
            }
            Err(_) => {}
        }

        if installed_version(adb, serial).await? != Some(HELPER_VERSION_CODE) {
            install(adb, serial).await?;
        }
        // The `am` process hosts the UiAutomation connection, so it has to outlive this adb
        // session: `-w` keeps it running, `nohup` detaches it.
        adb.shell(
            serial,
            &format!("nohup am instrument -w {INSTRUMENTATION} >/dev/null 2>&1 &"),
        )
        .await?;

        let deadline = Instant::now() + START_TIMEOUT;
        loop {
            match helper.version().await {
                Ok(_) => return Ok(helper),
                Err(e) if Instant::now() >= deadline => {
                    return Err(Error::HelperUnavailable {
                        reason: format!(
                            "not running {}s after start ({e})",
                            START_TIMEOUT.as_secs()
                        ),
                    });
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    }

    pub async fn version(&self) -> Result<u64> {
        let response = self.call(json!({ "cmd": "ping" })).await?;
        Ok(response["version_code"].as_u64().unwrap_or_default())
    }

    /// Window roots and the on-screen window list.
    pub async fn tree(&self) -> Result<(Vec<RawNode>, Vec<WindowInfo>)> {
        let mut response = self.call(json!({ "cmd": "tree" })).await?;
        Ok((
            decode(response["roots"].take())?,
            decode(response["windows"].take())?,
        ))
    }

    /// Injects input through UiAutomation: a few ms instead of ~120 ms for `adb shell input`.
    pub async fn input(&self, input: &Input) -> Result<()> {
        let request = match input {
            Input::Tap { x, y } => json!({ "cmd": "tap", "x": x, "y": y }),
            Input::Swipe {
                from,
                to,
                duration_ms,
                hold_ms,
            } => json!({
                "cmd": "swipe",
                "x1": from.0, "y1": from.1, "x2": to.0, "y2": to.1,
                "duration_ms": duration_ms,
                "hold_ms": hold_ms,
            }),
            Input::Key { name } => json!({ "cmd": "key", "key": name }),
            Input::SetText { text } => json!({ "cmd": "set_text", "text": text }),
        };
        self.call(request).await.map(drop)
    }

    /// Waits until no accessibility events arrived for `quiet`; `false` if `timeout` hit first.
    pub async fn wait_idle(&self, quiet: Duration, timeout: Duration) -> Result<bool> {
        let timeout = timeout.min(REQUEST_TIMEOUT - Duration::from_secs(1));
        let response = self
            .call(json!({
                "cmd": "wait_idle",
                "quiet_ms": quiet.as_millis() as u64,
                "timeout_ms": timeout.as_millis() as u64,
            }))
            .await?;
        Ok(response["idle"].as_bool().unwrap_or(false))
    }

    async fn call(&self, request: Value) -> Result<Value> {
        let cmd = request["cmd"].as_str().unwrap_or_default().to_owned();
        let exchange = async {
            let stream = TcpStream::connect(("127.0.0.1", self.port)).await?;
            let (read, mut write) = stream.into_split();
            write.write_all(format!("{request}\n").as_bytes()).await?;
            let mut line = String::new();
            BufReader::new(read).read_line(&mut line).await?;
            Ok::<_, std::io::Error>(line)
        };
        let line = tokio::time::timeout(REQUEST_TIMEOUT, exchange)
            .await
            .map_err(|_| unavailable(format!("`{cmd}` timed out")))?
            .map_err(unavailable)?;
        // adb accepts the forwarded connection even when nothing listens on the device and then
        // closes it, so an empty response means the helper isn't running.
        if line.is_empty() {
            return Err(unavailable("not running"));
        }
        let response: Value = serde_json::from_str(&line).map_err(|e| Error::Parse {
            tool: "mdh helper".into(),
            detail: e.to_string(),
        })?;
        if response["ok"] == true {
            Ok(response)
        } else {
            Err(Error::HelperCommand {
                cmd,
                message: response["error"]
                    .as_str()
                    .unwrap_or("unknown error")
                    .to_owned(),
            })
        }
    }
}

/// Stops the helper, releasing the device's UiAutomation connection.
pub(crate) async fn stop(adb: &Adb, serial: &str) -> Result<()> {
    adb.shell(serial, &format!("am force-stop {PACKAGE}"))
        .await
        .map(drop)
}

fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|e| Error::Parse {
        tool: "mdh helper".into(),
        detail: e.to_string(),
    })
}

fn unavailable(reason: impl ToString) -> Error {
    Error::HelperUnavailable {
        reason: reason.to_string(),
    }
}

/// Reuses this device's existing forward (they outlive mdh invocations) or creates one.
async fn forward(adb: &Adb, serial: &str) -> Result<u16> {
    let list = adb.on(serial, &["forward", "--list"]).await?;
    if let Some(port) = parse_forward(&list, serial) {
        return Ok(port);
    }
    let out = adb.on(serial, &["forward", "tcp:0", SOCKET]).await?;
    out.trim().parse().map_err(|_| Error::Parse {
        tool: "adb forward".into(),
        detail: out,
    })
}

/// Finds `<serial> tcp:<port> localabstract:mdh-helper` in `adb forward --list`.
fn parse_forward(list: &str, serial: &str) -> Option<u16> {
    list.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let (s, local, remote) = (fields.next()?, fields.next()?, fields.next()?);
        if s != serial || remote != SOCKET {
            return None;
        }
        local.strip_prefix("tcp:")?.parse().ok()
    })
}

async fn installed_version(adb: &Adb, serial: &str) -> Result<Option<u64>> {
    let out = adb
        .shell(serial, &format!("dumpsys package {PACKAGE}"))
        .await?;
    Ok(parse_version_code(&out))
}

/// Extracts `versionCode=N` from `dumpsys package`; `None` when the package isn't installed.
fn parse_version_code(dumpsys: &str) -> Option<u64> {
    let rest = &dumpsys[dumpsys.find("versionCode=")? + "versionCode=".len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

async fn install(adb: &Adb, serial: &str) -> Result<()> {
    let path = std::env::temp_dir().join(format!("mdh-helper-{HELPER_VERSION_CODE}.apk"));
    std::fs::write(&path, APK)?;
    let path = path.to_string_lossy();
    let args: [&str; 4] = ["install", "-r", "-t", &path];
    let mut result = adb.on(serial, &args).await;
    if let Err(Error::CommandFailed { stderr, .. }) = &result
        && blocked_by_installed_helper(stderr)
    {
        // The helper keeps no data, so the one in the way goes. If it can't be uninstalled, the
        // install is refused once more and that is what gets reported.
        let _ = adb.on(serial, &["uninstall", PACKAGE]).await;
        result = adb.on(serial, &args).await;
    }
    result.map(drop).map_err(install_failed)
}

/// Whether `adb install -r` was refused because of the helper already on the device: one signed
/// with another key (a local debug build) or a newer one (another mdh's, which this binary can't
/// use: host and helper change together). Neither can be replaced in place; `-d` allows a
/// downgrade only where the package or the system image is debuggable.
fn blocked_by_installed_helper(stderr: &str) -> bool {
    super::install::parse_failure(stderr).is_some_and(|(reason, _)| {
        matches!(
            reason.as_str(),
            "INSTALL_FAILED_UPDATE_INCOMPATIBLE" | "INSTALL_FAILED_VERSION_DOWNGRADE"
        )
    })
}

/// Android's refusal as an error about the helper: the caller asked to read the screen, not to
/// install anything, and what fixes an app's install doesn't fix this one.
fn install_failed(e: Error) -> Error {
    if let Error::CommandFailed { stderr, .. } = &e
        && let Some((reason, detail)) = super::install::parse_failure(stderr)
    {
        return Error::HelperInstallFailed { reason, detail };
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_this_devices_forward() {
        let list = "\
emulator-5556 tcp:40001 localabstract:mdh-helper
emulator-5554 tcp:49557 localabstract:mdh-helper
emulator-5554 tcp:8080 tcp:8080
";
        assert_eq!(parse_forward(list, "emulator-5554"), Some(49557));
        assert_eq!(parse_forward(list, "R58M123ABC"), None);
    }

    #[test]
    fn reads_version_code() {
        let dumpsys = "    versionCode=12 minSdk=26 targetSdk=36\n    versionName=1";
        assert_eq!(parse_version_code(dumpsys), Some(12));
        assert_eq!(
            parse_version_code("Unable to find package: dev.mdh.helper"),
            None
        );
    }

    /// Captured `adb install` output. `version_downgrade_api36` is what a binary embedding
    /// version 5 got where another checkout's mdh had installed 6.
    fn adb_install(name: &str) -> String {
        let path = format!(
            "{}/../../fixtures/android/adb/install_{name}.txt",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read_to_string(path).unwrap()
    }

    fn failed(stderr: &str) -> Error {
        Error::CommandFailed {
            command: "adb install".into(),
            code: Some(1),
            stderr: stderr.into(),
        }
    }

    #[test]
    fn a_newer_or_differently_signed_helper_is_in_the_way() {
        for capture in ["version_downgrade_api36", "signature_mismatch"] {
            let stderr = adb_install(capture);
            assert!(blocked_by_installed_helper(&stderr), "{capture}");
        }
    }

    #[test]
    fn other_failures_leave_the_installed_helper_alone() {
        let stderr = adb_install("no_matching_abis");
        assert!(!blocked_by_installed_helper(&stderr));
        assert!(!blocked_by_installed_helper("adb: device offline"));
    }

    #[test]
    fn a_refused_install_names_the_helper_and_what_to_do() {
        let e = install_failed(failed(&adb_install("version_downgrade_api36")));
        assert_eq!(e.code(), mdh_core::ErrorCode::HelperUnavailable);
        assert_eq!(
            e.to_string(),
            "could not install the on-device helper: INSTALL_FAILED_VERSION_DOWNGRADE (Downgrade detected: \
             Update version code 5 is older than current 6)"
        );
        assert!(
            e.hint().contains("`adb uninstall dev.mdh.helper`"),
            "{}",
            e.hint()
        );
    }

    #[test]
    fn a_failure_that_isnt_androids_stays_as_it_is() {
        let e = install_failed(failed("adb: device offline"));
        assert!(matches!(e, Error::CommandFailed { .. }), "{e}");
    }
}
