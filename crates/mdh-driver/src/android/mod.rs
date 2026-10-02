//! Android backend built on the platform-tools CLIs and the on-device helper.

mod adb;
mod helper;
mod sdk;
mod uiautomator;

pub use adb::Adb;
pub use helper::{HELPER_VERSION_CODE, Helper};
pub use sdk::AndroidSdk;
pub use uiautomator::parse_hierarchy;

use std::time::Duration;

use async_trait::async_trait;
use mdh_core::ui::{RawTree, TreeSource};
use mdh_core::{Device, Error, Platform, Result};

use crate::Driver;

pub struct AndroidDriver {
    adb: Adb,
}

impl AndroidDriver {
    pub fn new(sdk: &AndroidSdk) -> Self {
        Self {
            adb: Adb::new(sdk.adb.clone()),
        }
    }

    pub fn adb(&self) -> &Adb {
        &self.adb
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
        })
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
        match Helper::ensure(&self.adb, &device.id).await {
            Ok(helper) => Ok(RawTree {
                roots: helper.tree().await?,
                source: TreeSource::Helper,
            }),
            Err(Error::HelperUnavailable { .. }) => self.dump_with_uiautomator(device).await,
            Err(e) => Err(e),
        }
    }
}
