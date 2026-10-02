//! Core types shared across mobile-dev-harness crates.

mod device;
mod error;

pub use device::{Device, DeviceState, Platform};
pub use error::{Error, Result};
