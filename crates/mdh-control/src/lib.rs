//! Control: drives a device for the other domains (verify, perf, compat, visual) and for agents.
//!
//! Owns device selection, observation of the current screen, input and the app lifecycle. The
//! session engine (session-stable refs, ref and selector targeting, wait-for-stable, diffs after
//! actions) lands here next.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use mdh_core::output::Timings;
use mdh_core::ui::{ScreenInfo, TreeSource};
use mdh_core::{Device, Input, LaunchInfo, Result};
use mdh_driver::Driver;
use mdh_driver::android::{AndroidDriver, AndroidSdk};
use mdh_observe::{Jpeg, RefTable, UiTree, compress, render, screenshot_jpeg};
use serde::Serialize;

/// The driver and the device all control operations work on.
pub struct Control {
    driver: Arc<dyn Driver>,
    device: Device,
}

#[derive(Debug, Serialize)]
pub struct Observation {
    pub screen: ScreenInfo,
    pub source: TreeSource,
    /// The compact text form agents read.
    pub text: String,
    pub tree: UiTree,
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
        Ok(Self {
            driver: Arc::new(driver),
            device,
        })
    }

    pub fn device(&self) -> &Device {
        &self.device
    }

    pub async fn observe(&self, timings: &mut Timings) -> Result<Observation> {
        let started = Instant::now();
        let (raw, activity) = tokio::join!(
            self.driver.ui_tree(&self.device),
            self.driver.foreground_activity(&self.device)
        );
        let raw = raw?;
        timings.record("observe", started);

        let mut tree = compress(&raw.roots);
        RefTable::default().assign(&mut tree);
        Ok(Observation {
            screen: ScreenInfo::new(activity?, tree.screen, &raw.windows),
            source: raw.source,
            text: render(&tree),
            tree,
        })
    }

    /// Saves a JPEG downscaled to `max_edge` at `output`.
    pub async fn screenshot(
        &self,
        output: PathBuf,
        max_edge: u32,
        timings: &mut Timings,
    ) -> Result<Screenshot> {
        let started = Instant::now();
        let png = self.driver.screenshot(&self.device).await?;
        timings.record("capture", started);
        let image = screenshot_jpeg(&png, max_edge, 80)?;
        std::fs::write(&output, &image.bytes)?;
        Ok(Screenshot {
            path: output,
            image,
        })
    }

    pub async fn input(&self, input: &Input) -> Result<()> {
        self.driver.input(&self.device, input).await
    }

    pub async fn launch(&self, app: &str) -> Result<LaunchInfo> {
        self.driver.launch(&self.device, app).await
    }

    pub async fn stop(&self, package: &str) -> Result<()> {
        self.driver.stop(&self.device, package).await
    }

    pub async fn install(&self, apk: &Path, grant_permissions: bool) -> Result<()> {
        self.driver
            .install(&self.device, apk, grant_permissions)
            .await
    }
}
