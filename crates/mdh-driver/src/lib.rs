//! Device drivers for mobile-dev-harness.

pub mod android;
mod process;

use async_trait::async_trait;
use mdh_core::{Device, Platform, Result};

/// A platform backend that discovers and controls devices.
///
/// Only discovery exists so far; install, launch, UI, input and logs land in M1.
#[async_trait]
pub trait Driver: Send + Sync {
    fn platform(&self) -> Platform;

    async fn devices(&self) -> Result<Vec<Device>>;
}
