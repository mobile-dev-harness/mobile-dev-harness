//! Device drivers for mobile-dev-harness.

pub mod android;
mod process;

use async_trait::async_trait;
use mdh_core::ui::RawTree;
use mdh_core::{Device, DeviceState, Error, Platform, Result};

/// A platform backend that discovers and controls devices.
///
/// Install, launch, input and logs land later in M1.
#[async_trait]
pub trait Driver: Send + Sync {
    fn platform(&self) -> Platform;

    async fn devices(&self) -> Result<Vec<Device>>;

    async fn ui_tree(&self, device: &Device) -> Result<RawTree>;
}

/// Picks the device to work on: the requested one, or the only online device.
pub fn select_device(devices: Vec<Device>, requested: Option<&str>) -> Result<Device> {
    let mut online: Vec<Device> = devices
        .into_iter()
        .filter(|d| d.state == DeviceState::Online)
        .collect();
    if let Some(id) = requested {
        return online
            .into_iter()
            .find(|d| d.id == id)
            .ok_or_else(|| Error::DeviceNotFound { id: id.to_owned() });
    }
    match online.len() {
        0 => Err(Error::NoDevice),
        1 => Ok(online.remove(0)),
        _ => Err(Error::AmbiguousDevice {
            candidates: online.into_iter().map(|d| d.id).collect(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: &str, state: DeviceState) -> Device {
        Device {
            id: id.into(),
            platform: Platform::Android,
            state,
            model: None,
            is_emulator: true,
        }
    }

    #[test]
    fn selects_the_only_online_device() {
        let devices = vec![
            device("a", DeviceState::Offline),
            device("b", DeviceState::Online),
        ];
        assert_eq!(select_device(devices, None).unwrap().id, "b");
    }

    #[test]
    fn refuses_to_guess_between_devices() {
        let devices = vec![
            device("a", DeviceState::Online),
            device("b", DeviceState::Online),
        ];
        assert!(matches!(
            select_device(devices.clone(), None),
            Err(Error::AmbiguousDevice { .. })
        ));
        assert_eq!(select_device(devices, Some("a")).unwrap().id, "a");
    }

    #[test]
    fn requested_device_must_be_online() {
        let devices = vec![device("a", DeviceState::Unauthorized)];
        assert!(matches!(
            select_device(devices, Some("a")),
            Err(Error::DeviceNotFound { .. })
        ));
    }
}
