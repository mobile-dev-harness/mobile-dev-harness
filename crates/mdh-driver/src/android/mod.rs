//! Android backend built on the platform-tools CLIs and the on-device helper.

mod adb;
mod am;
mod helper;
mod sdk;
mod uiautomator;

pub use adb::{Adb, shell_quote};
pub use helper::{HELPER_VERSION_CODE, Helper};
pub use sdk::AndroidSdk;
pub use uiautomator::parse_hierarchy;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use mdh_core::ui::{RawTree, TreeSource};
use mdh_core::{Device, Error, Input, LaunchInfo, Platform, Result};

use crate::Driver;

pub struct AndroidDriver {
    adb: Adb,
    /// Helpers already verified in this process, by device serial.
    helpers: Mutex<HashMap<String, Arc<Helper>>>,
}

impl AndroidDriver {
    pub fn new(sdk: &AndroidSdk) -> Self {
        Self {
            adb: Adb::new(sdk.adb.clone()),
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
            Input::Swipe {
                from,
                to,
                duration_ms,
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
        self.adb.devices().await
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
        let mut args = vec!["install", "-r", "-t"];
        if grant_permissions {
            args.push("-g");
        }
        args.push(&apk);
        self.adb
            .on_with_timeout(&device.id, &args, Duration::from_secs(300))
            .await
            .map(drop)
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
