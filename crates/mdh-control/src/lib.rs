//! Control: drives a device for the other domains (verify, perf, compat, visual) and for agents.
//!
//! [`Control`] wraps a driver and a device with stateless operations. [`Session`] adds what agents
//! need across calls: session-stable refs, ref and selector targeting, waiting for the UI to
//! settle after actions, diffs against what the agent last saw, and a recording of every step.

mod action;
mod run;
mod session;
mod settle;
mod target;
mod text;

pub use action::{ActOutcome, Action, Direction};
pub use run::{InstallReport, RunOptions, RunReport, new_run_dir};
pub use session::{LogsReport, Observation, RecordedStep, Session, SessionSummary};
pub use target::{Selector, Target, TextMatch, find_all, find_matches, find_one, selector_for};
pub use text::launch_text;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use mdh_core::output::Timings;
use mdh_core::ui::{ScreenInfo, TreeSource, WindowInfo};
use mdh_core::{Device, Input, LaunchInfo, LogEntry, Result};
use mdh_driver::Driver;
use mdh_driver::android::{AndroidDriver, AndroidSdk};
use mdh_observe::{Jpeg, UiTree, compress, screenshot_jpeg};
use serde::Serialize;

/// The driver and the device all control operations work on.
pub struct Control {
    driver: Arc<dyn Driver>,
    device: Device,
}

/// The compressed tree and screen info at one moment. Refs are not assigned yet.
pub struct Snapshot {
    pub tree: UiTree,
    pub screen: ScreenInfo,
    pub source: TreeSource,
}

impl Snapshot {
    fn new(
        mut tree: UiTree,
        windows: &[WindowInfo],
        activity: Option<String>,
        source: TreeSource,
    ) -> Self {
        let screen = ScreenInfo::new(activity, tree.screen, windows);
        tree.mark_obscured(&screen.obstructions);
        Self {
            tree,
            screen,
            source,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Screenshot {
    pub path: PathBuf,
    #[serde(flatten)]
    pub image: Jpeg,
}

impl Control {
    /// Connects to the requested device, or the only online one. Android is the only platform so far.
    pub async fn connect(requested_device: Option<&str>) -> Result<Self> {
        let driver = AndroidDriver::new(&AndroidSdk::locate()?);
        let device = mdh_driver::select_device(driver.devices().await?, requested_device)?;
        Ok(Self::new(Arc::new(driver), device))
    }

    pub fn new(driver: Arc<dyn Driver>, device: Device) -> Self {
        Self { driver, device }
    }

    pub fn device(&self) -> &Device {
        &self.device
    }

    pub async fn snapshot(&self) -> Result<Snapshot> {
        let (tree, activity) =
            tokio::join!(self.tree(), self.driver.foreground_activity(&self.device));
        let (tree, windows, source) = tree?;
        Ok(Snapshot::new(tree, &windows, activity?, source))
    }

    /// Just the tree, for polling.
    async fn tree(&self) -> Result<(UiTree, Vec<WindowInfo>, TreeSource)> {
        let raw = self.driver.ui_tree(&self.device).await?;
        Ok((compress(&raw.roots), raw.windows, raw.source))
    }

    /// A JPEG of the screen downscaled to `max_edge`.
    pub async fn capture(&self, max_edge: u32, timings: &mut Timings) -> Result<Jpeg> {
        let started = Instant::now();
        let png = self.driver.screenshot(&self.device).await?;
        timings.record("capture", started);
        screenshot_jpeg(&png, max_edge, 80)
    }

    /// Saves a JPEG downscaled to `max_edge` at `output`.
    pub async fn screenshot(
        &self,
        output: PathBuf,
        max_edge: u32,
        timings: &mut Timings,
    ) -> Result<Screenshot> {
        let image = self.capture(max_edge, timings).await?;
        std::fs::write(&output, &image.bytes)?;
        Ok(Screenshot {
            path: output,
            image,
        })
    }

    pub async fn input(&self, input: &Input) -> Result<()> {
        self.driver.input(&self.device, input).await
    }

    async fn wait_idle(&self, quiet: Duration, timeout: Duration) -> Result<bool> {
        self.driver.wait_idle(&self.device, quiet, timeout).await
    }

    pub async fn launch(&self, app: &str) -> Result<LaunchInfo> {
        self.driver.launch(&self.device, app).await
    }

    pub async fn open_uri(&self, uri: &str, package: Option<&str>) -> Result<LaunchInfo> {
        self.driver.open_uri(&self.device, uri, package).await
    }

    pub async fn clear_data(&self, package: &str) -> Result<()> {
        self.driver.clear_data(&self.device, package).await
    }

    pub async fn set_permission(
        &self,
        package: &str,
        permission: &str,
        granted: bool,
    ) -> Result<()> {
        self.driver
            .set_permission(&self.device, package, permission, granted)
            .await
    }

    pub async fn stop(&self, package: &str) -> Result<()> {
        self.driver.stop(&self.device, package).await
    }

    pub async fn uninstall(&self, package: &str) -> Result<()> {
        self.driver.uninstall(&self.device, package).await
    }

    pub async fn abis(&self) -> Result<Vec<String>> {
        self.driver.abis(&self.device).await
    }

    pub fn sdk_root(&self) -> Option<PathBuf> {
        self.driver.sdk_root()
    }

    pub async fn installed_path(&self, package: &str) -> Result<Option<String>> {
        self.driver.installed_path(&self.device, package).await
    }

    pub async fn install(&self, apk: &Path, grant_permissions: bool) -> Result<()> {
        self.driver
            .install(&self.device, apk, grant_permissions)
            .await
    }

    pub async fn clock_ms(&self) -> Result<u64> {
        self.driver.clock_ms(&self.device).await
    }

    pub async fn logs(&self, since_ms: u64) -> Result<Vec<LogEntry>> {
        self.driver.logs(&self.device, since_ms).await
    }

    pub async fn pids(&self, packages: &[String]) -> Result<Vec<u32>> {
        self.driver.pids(&self.device, packages).await
    }

    /// Stops background helpers on the device so other UiAutomation clients can run.
    pub async fn release(&self) -> Result<()> {
        self.driver.release(&self.device).await
    }
}
