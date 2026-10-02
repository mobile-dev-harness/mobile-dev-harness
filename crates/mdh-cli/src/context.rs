use mdh_core::{Device, Result};
use mdh_driver::Driver;
use mdh_driver::android::{AndroidDriver, AndroidSdk};

/// The driver and the selected device every device command works on.
pub struct Context {
    pub driver: AndroidDriver,
    pub device: Device,
}

impl Context {
    pub async fn new(requested_device: Option<&str>) -> Result<Self> {
        let driver = AndroidDriver::new(&AndroidSdk::locate()?);
        let device = mdh_driver::select_device(driver.devices().await?, requested_device)?;
        Ok(Self { driver, device })
    }
}
