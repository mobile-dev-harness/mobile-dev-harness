//! Android backend built on the platform-tools CLIs.

mod adb;
mod sdk;

pub use adb::Adb;
pub use sdk::AndroidSdk;

use async_trait::async_trait;
use mdh_core::{Device, Platform, Result};

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
}

#[async_trait]
impl Driver for AndroidDriver {
    fn platform(&self) -> Platform {
        Platform::Android
    }

    async fn devices(&self) -> Result<Vec<Device>> {
        self.adb.devices().await
    }
}
