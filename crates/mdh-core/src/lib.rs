//! Core types shared across mobile-dev-harness crates.

mod device;
mod error;
pub mod output;

pub use device::{Device, DeviceState, Platform};
pub use error::{Error, ErrorCode, Result};
