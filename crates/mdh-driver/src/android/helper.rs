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
    /// Connects to the helper on `serial`, installing, upgrading or starting it as needed.
    pub async fn ensure(adb: &Adb, serial: &str) -> Result<Helper> {
        let helper = Helper {
            port: forward(adb, serial).await?,
        };
        match helper.version().await {
            Ok(HELPER_VERSION_CODE) => return Ok(helper),
            Ok(_stale) => {
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
    match adb.on(serial, &["install", "-r", "-t", &path]).await {
        // A helper signed with another key (e.g. a local debug build) can't be upgraded in place.
        Err(Error::CommandFailed { stderr, .. })
            if stderr.contains("INSTALL_FAILED_UPDATE_INCOMPATIBLE") =>
        {
            adb.on(serial, &["uninstall", PACKAGE]).await?;
            adb.on(serial, &["install", "-t", &path]).await.map(drop)
        }
        result => result.map(drop),
    }
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
}
