//! Choosing the device to work on: an explicit choice, the project's default, the only device
//! online, an emulator over a phone. Never a guess between equals, and never an emulator started
//! without someone saying yes.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use mdh_core::{Avd, Device, DeviceState, Error, Result};
use mdh_driver::Driver;
use mdh_driver::android::{AndroidDriver, AndroidSdk};
use serde::{Deserialize, Serialize};

use crate::Control;

/// Where the project's default device is kept, relative to the working directory. Local to each
/// developer (device names differ), so `.mdh/` keeps it out of git.
pub const DEFAULT_DEVICE_FILE: &str = ".mdh/device.json";

/// The device a project uses unless told otherwise: an emulator by its AVD (serials change between
/// runs), a phone by its serial.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefaultDevice {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial: Option<String>,
}

impl DefaultDevice {
    pub fn of(device: &Device) -> Self {
        match &device.avd {
            Some(avd) => DefaultDevice {
                avd: Some(avd.clone()),
                serial: None,
            },
            None => DefaultDevice {
                avd: None,
                serial: Some(device.id.clone()),
            },
        }
    }

    pub fn load(path: &Path) -> Option<Self> {
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_vec_pretty(self).expect("serializable"))?;
        Ok(())
    }

    fn matches(&self, d: &Device) -> bool {
        self.serial.as_deref() == Some(d.id.as_str()) || (self.avd.is_some() && self.avd == d.avd)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Use this device; `why` explains a choice that isn't obvious.
    Ready { device: Device, why: Option<String> },
    /// The user named an emulator that isn't running: start it.
    Start { avd: String },
    /// Several devices fit equally; someone has to choose.
    Choose { devices: Vec<Device> },
    /// Nothing is online; these emulators could be started, the suggested one first.
    Offline { avds: Vec<Avd> },
}

/// Decides which device to use. `requested` is a serial or an AVD name.
pub async fn resolve(
    driver: &dyn Driver,
    requested: Option<&str>,
    default: Option<&DefaultDevice>,
) -> Result<Resolution> {
    let online: Vec<Device> = driver
        .devices()
        .await?
        .into_iter()
        .filter(|d| d.state == DeviceState::Online)
        .collect();
    if let Some(id) = requested {
        if let Some(d) = online
            .iter()
            .find(|d| d.id == id || d.avd.as_deref() == Some(id))
        {
            return Ok(Resolution::Ready {
                device: d.clone(),
                why: None,
            });
        }
        if driver.avds().await?.iter().any(|a| a.name == id) {
            return Ok(Resolution::Start { avd: id.to_owned() });
        }
        return Err(Error::DeviceNotFound { id: id.to_owned() });
    }
    if let Some(default) = default
        && let Some(d) = online.iter().find(|d| default.matches(d))
    {
        let why = (online.len() > 1)
            .then(|| format!("{} is this project's default device", d.describe()));
        return Ok(Resolution::Ready {
            device: d.clone(),
            why,
        });
    }
    match online.len() {
        0 => {
            let mut avds = driver.avds().await?;
            // The project's emulator first, then the newest system images.
            avds.sort_by_key(|a| {
                let preferred = default.and_then(|d| d.avd.as_deref()) == Some(a.name.as_str());
                (!preferred, std::cmp::Reverse(a.api))
            });
            if avds.is_empty() {
                return Err(Error::NoDevice { avds: Vec::new() });
            }
            Ok(Resolution::Offline { avds })
        }
        1 => Ok(Resolution::Ready {
            device: online[0].clone(),
            why: None,
        }),
        _ => {
            // Functional verification prefers an emulator: resettable and the same every run.
            let emulators: Vec<&Device> = online.iter().filter(|d| d.is_emulator).collect();
            if let [only] = emulators.as_slice() {
                return Ok(Resolution::Ready {
                    device: (*only).clone(),
                    why: Some(format!(
                        "using the emulator {}; pass --device to use a phone instead",
                        only.describe()
                    )),
                });
            }
            Ok(Resolution::Choose { devices: online })
        }
    }
}

/// Asks a person, when there is one at a terminal. Agents get errors listing the options instead
/// and ask their user.
pub trait Ask: Sync {
    /// Which of several devices to use, or `None` to cancel.
    fn choose(&self, devices: &[Device]) -> Option<usize>;
    /// Which emulator to start (the first is suggested), or `None` to start none.
    fn start(&self, avds: &[Avd]) -> Option<usize>;
    /// An emulator is starting; this can take a few minutes.
    fn starting(&self, _avd: &str) {}
}

pub struct ConnectOptions<'a> {
    /// Serial or AVD name.
    pub requested: Option<&'a str>,
    /// Where the project's default device is kept.
    pub default_file: &'a Path,
    pub ask: Option<&'a dyn Ask>,
    /// Start emulators without a window (CI).
    pub headless: bool,
}

/// A connection, and anything worth telling about how the device was chosen.
pub struct Connected {
    pub control: Control,
    pub notice: Option<String>,
    /// An emulator was started for this connection: nothing kept for its serial still applies.
    pub started: bool,
}

impl Control {
    /// Connects to the device [`resolve`] picks, asking through `ask` when it can't pick alone.
    /// Choices made by asking become the project's default.
    pub async fn connect_with(options: ConnectOptions<'_>) -> Result<Connected> {
        let driver: Arc<dyn Driver> = Arc::new(android()?);
        let default = DefaultDevice::load(options.default_file);
        let remember = |d: &Device| DefaultDevice::of(d).save(options.default_file);
        let mut started = false;
        let (device, notice) =
            match resolve(driver.as_ref(), options.requested, default.as_ref()).await? {
                Resolution::Ready { device, why } => (device, why),
                Resolution::Start { avd } => {
                    let (device, notice) =
                        start(driver.as_ref(), &avd, options.headless, options.ask).await?;
                    started = true;
                    (device, Some(notice))
                }
                Resolution::Choose { devices } => {
                    let picked = options.ask.and_then(|a| a.choose(&devices));
                    let Some(i) = picked.filter(|i| *i < devices.len()) else {
                        return Err(Error::AmbiguousDevice {
                            candidates: devices.iter().map(Device::describe).collect(),
                        });
                    };
                    remember(&devices[i])?;
                    (devices[i].clone(), None)
                }
                Resolution::Offline { avds } => {
                    let picked = options.ask.and_then(|a| a.start(&avds));
                    let Some(i) = picked.filter(|i| *i < avds.len()) else {
                        return Err(Error::NoDevice {
                            avds: avds.iter().map(Avd::describe).collect(),
                        });
                    };
                    let (device, notice) = start(
                        driver.as_ref(),
                        &avds[i].name,
                        options.headless,
                        options.ask,
                    )
                    .await?;
                    remember(&device)?;
                    started = true;
                    (device, Some(notice))
                }
            };
        Ok(Connected {
            control: Control::new(driver, device),
            notice,
            started,
        })
    }

    /// Connects without asking anyone, using the project default in the working directory.
    pub async fn connect(requested_device: Option<&str>) -> Result<Self> {
        Ok(Self::connect_with(ConnectOptions {
            requested: requested_device,
            default_file: Path::new(DEFAULT_DEVICE_FILE),
            ask: None,
            headless: false,
        })
        .await?
        .control)
    }
}

async fn start(
    driver: &dyn Driver,
    avd: &str,
    headless: bool,
    ask: Option<&dyn Ask>,
) -> Result<(Device, String)> {
    if let Some(a) = ask {
        a.starting(avd);
    }
    let started = Instant::now();
    let device = driver.start_emulator(avd, headless).await?;
    let notice = format!(
        "started the emulator {} in {} s; `mdh emulator stop` shuts it down",
        device.describe(),
        started.elapsed().as_secs()
    );
    Ok((device, notice))
}

fn android() -> Result<AndroidDriver> {
    Ok(AndroidDriver::new(&AndroidSdk::locate()?))
}

/// Starts `avd` (default: the project's emulator, else the newest system image), waits until it
/// has booted and makes it the project's default. Returns the device and what to tell.
pub async fn start_emulator(
    avd: Option<&str>,
    headless: bool,
    default_file: &Path,
) -> Result<(Device, String)> {
    let driver = android()?;
    let mut avds = driver.avds().await?;
    let default = DefaultDevice::load(default_file);
    avds.sort_by_key(|a| {
        let preferred = default.as_ref().and_then(|d| d.avd.as_deref()) == Some(a.name.as_str());
        (!preferred, std::cmp::Reverse(a.api))
    });
    let name = match avd {
        Some(n) if avds.iter().any(|a| a.name == n) => n.to_owned(),
        Some(n) => return Err(Error::DeviceNotFound { id: n.to_owned() }),
        None => avds
            .first()
            .map(|a| a.name.clone())
            .ok_or(Error::NoDevice { avds: Vec::new() })?,
    };
    if let Some(serial) = avds
        .iter()
        .find(|a| a.name == name)
        .and_then(|a| a.running.clone())
    {
        let device = driver
            .devices()
            .await?
            .into_iter()
            .find(|d| d.id == serial)
            .ok_or(Error::DeviceNotFound { id: serial })?;
        return Ok((
            device.clone(),
            format!("the emulator {} is already running", device.describe()),
        ));
    }
    let (device, notice) = start(&driver, &name, headless, None).await?;
    DefaultDevice::of(&device).save(default_file)?;
    Ok((device, notice))
}

/// Shuts down `serial`, else the project's default emulator, else the only emulator running.
pub async fn stop_emulator(serial: Option<&str>, default_file: &Path) -> Result<String> {
    let driver = android()?;
    let running: Vec<Device> = driver
        .devices()
        .await?
        .into_iter()
        .filter(|d| d.is_emulator && d.state == DeviceState::Online)
        .collect();
    let default = DefaultDevice::load(default_file);
    let target = match serial {
        Some(s) => running
            .iter()
            .find(|d| d.id == s || d.avd.as_deref() == Some(s)),
        None => running
            .iter()
            .find(|d| default.as_ref().is_some_and(|x| x.matches(d)))
            .or(if running.len() == 1 {
                running.first()
            } else {
                None
            }),
    };
    let Some(device) = target else {
        return Err(match (serial, running.len()) {
            (Some(s), _) => Error::DeviceNotFound { id: s.to_owned() },
            (None, 0) => Error::NoDevice { avds: Vec::new() },
            (None, _) => Error::AmbiguousDevice {
                candidates: running.iter().map(Device::describe).collect(),
            },
        });
    };
    driver.stop_emulator(device).await?;
    Ok(format!("stopped the emulator {}", device.describe()))
}

/// Makes `id` (a serial, or an AVD running or not) the project's default device.
pub async fn use_device(id: &str, default_file: &Path) -> Result<String> {
    let driver = android()?;
    let device = driver
        .devices()
        .await?
        .into_iter()
        .find(|d| d.id == id || d.avd.as_deref() == Some(id));
    let default = match device {
        Some(d) => DefaultDevice::of(&d),
        None if driver.avds().await?.iter().any(|a| a.name == id) => DefaultDevice {
            avd: Some(id.to_owned()),
            serial: None,
        },
        None => return Err(Error::DeviceNotFound { id: id.to_owned() }),
    };
    default.save(default_file)?;
    Ok(format!(
        "{id} is now this project's default device ({})",
        default_file.display()
    ))
}

/// What `mdh devices` shows: devices online, emulators that could be started, the default.
#[derive(Debug, Clone, Serialize)]
pub struct Inventory {
    pub devices: Vec<Device>,
    pub avds: Vec<Avd>,
    pub default: Option<DefaultDevice>,
}

impl Inventory {
    pub async fn load(default_file: &Path) -> Result<Inventory> {
        let driver = android()?;
        Ok(Inventory {
            devices: driver.devices().await?,
            avds: driver.avds().await.unwrap_or_default(),
            default: DefaultDevice::load(default_file),
        })
    }

    pub fn text(&self) -> String {
        let mut lines = Vec::new();
        let is_default = |d: &Device| self.default.as_ref().is_some_and(|x| x.matches(d));
        if self.devices.is_empty() {
            lines.push("No devices connected.".to_owned());
        }
        for d in &self.devices {
            let state = match &d.state {
                DeviceState::Online => "online",
                DeviceState::Offline => "offline",
                DeviceState::Unauthorized => {
                    "unauthorized (accept the USB debugging prompt on the phone)"
                }
                DeviceState::Other(s) => s,
            };
            let kind = if d.is_emulator { "emulator" } else { "device" };
            let name = d.avd.as_deref().or(d.model.as_deref()).unwrap_or("-");
            let api = d.api.map(|a| format!("API {a}")).unwrap_or_default();
            let mark = if is_default(d) { "  (default)" } else { "" };
            lines.push(format!(
                "{:<20} {kind:<9} {name:<24} {api:<7} {state}{mark}",
                d.id
            ));
        }
        let stopped: Vec<&Avd> = self.avds.iter().filter(|a| a.running.is_none()).collect();
        if !stopped.is_empty() {
            lines.push(String::new());
            lines.push("Emulators that can be started (`mdh emulator start <name>`):".to_owned());
            for a in stopped {
                let mark = if self.default.as_ref().and_then(|d| d.avd.as_deref())
                    == Some(a.name.as_str())
                {
                    "  (default)"
                } else {
                    ""
                };
                lines.push(format!("  {}{mark}", a.describe()));
            }
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use mdh_core::ui::RawTree;
    use mdh_core::{Input, LaunchInfo, Platform};

    struct Fake {
        devices: Vec<Device>,
        avds: Vec<Avd>,
    }

    #[async_trait]
    impl Driver for Fake {
        fn platform(&self) -> Platform {
            Platform::Android
        }
        async fn devices(&self) -> Result<Vec<Device>> {
            Ok(self.devices.clone())
        }
        async fn avds(&self) -> Result<Vec<Avd>> {
            Ok(self.avds.clone())
        }
        async fn ui_tree(&self, _: &Device) -> Result<RawTree> {
            unreachable!()
        }
        async fn foreground_activity(&self, _: &Device) -> Result<Option<String>> {
            unreachable!()
        }
        async fn input(&self, _: &Device, _: &Input) -> Result<()> {
            unreachable!()
        }
        async fn screenshot(&self, _: &Device) -> Result<Vec<u8>> {
            unreachable!()
        }
        async fn install(&self, _: &Device, _: &Path, _: bool) -> Result<()> {
            unreachable!()
        }
        async fn launch(&self, _: &Device, _: &str) -> Result<LaunchInfo> {
            unreachable!()
        }
        async fn stop(&self, _: &Device, _: &str) -> Result<()> {
            unreachable!()
        }
    }

    fn device(id: &str, avd: Option<&str>, state: DeviceState) -> Device {
        Device {
            id: id.into(),
            platform: Platform::Android,
            state,
            model: Some("Pixel".into()),
            is_emulator: avd.is_some(),
            avd: avd.map(str::to_owned),
            api: Some(36),
            manufacturer: None,
        }
    }

    fn avd(name: &str, api: u32) -> Avd {
        Avd {
            name: name.into(),
            api: Some(api),
            running: None,
        }
    }

    async fn pick(
        fake: &Fake,
        requested: Option<&str>,
        default: Option<&DefaultDevice>,
    ) -> Resolution {
        resolve(fake, requested, default).await.unwrap()
    }

    #[tokio::test]
    async fn the_only_device_and_an_emulator_over_a_phone() {
        let phone = device("R5CT", None, DeviceState::Online);
        let emu = device("emulator-5554", Some("Pixel_9"), DeviceState::Online);
        let off = device("ZY22", None, DeviceState::Unauthorized);
        let fake = Fake {
            devices: vec![phone.clone(), off],
            avds: vec![],
        };
        assert!(
            matches!(pick(&fake, None, None).await, Resolution::Ready { device, why: None } if device == phone)
        );
        let fake = Fake {
            devices: vec![phone.clone(), emu.clone()],
            avds: vec![],
        };
        let Resolution::Ready { device, why } = pick(&fake, None, None).await else {
            panic!()
        };
        assert_eq!(device, emu);
        assert!(why.unwrap().contains("pass --device to use a phone"));
    }

    #[tokio::test]
    async fn defaults_and_requests_by_serial_or_avd() {
        let a = device("emulator-5554", Some("Pixel_9"), DeviceState::Online);
        let b = device("emulator-5556", Some("Tablet"), DeviceState::Online);
        let fake = Fake {
            devices: vec![a.clone(), b.clone()],
            avds: vec![avd("Pixel_9", 36), avd("Tablet", 34), avd("Old", 30)],
        };
        assert!(
            matches!(pick(&fake, None, None).await, Resolution::Choose { devices } if devices.len() == 2)
        );
        let default = DefaultDevice::of(&b);
        assert_eq!(default.avd.as_deref(), Some("Tablet"));
        assert!(
            matches!(pick(&fake, None, Some(&default)).await, Resolution::Ready { device, .. } if device == b)
        );
        assert!(
            matches!(pick(&fake, Some("Pixel_9"), None).await, Resolution::Ready { device, .. } if device == a)
        );
        assert_eq!(
            pick(&fake, Some("Old"), None).await,
            Resolution::Start { avd: "Old".into() }
        );
        assert!(matches!(
            resolve(&fake, Some("nope"), None).await,
            Err(Error::DeviceNotFound { .. })
        ));
    }

    #[tokio::test]
    async fn nothing_online_suggests_the_default_then_the_newest() {
        let fake = Fake {
            devices: vec![],
            avds: vec![avd("Old", 30), avd("New", 36), avd("Mine", 33)],
        };
        let names = |r: Resolution| match r {
            Resolution::Offline { avds } => avds.into_iter().map(|a| a.name).collect::<Vec<_>>(),
            other => panic!("{other:?}"),
        };
        assert_eq!(names(pick(&fake, None, None).await), ["New", "Mine", "Old"]);
        let default = DefaultDevice {
            avd: Some("Mine".into()),
            serial: None,
        };
        assert_eq!(
            names(pick(&fake, None, Some(&default)).await),
            ["Mine", "New", "Old"]
        );
        let empty = Fake {
            devices: vec![],
            avds: vec![],
        };
        assert!(matches!(
            resolve(&empty, None, None).await,
            Err(Error::NoDevice { .. })
        ));
    }
}
