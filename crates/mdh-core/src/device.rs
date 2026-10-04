use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Android,
    Ios,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceState {
    Online,
    Offline,
    Unauthorized,
    /// A state reported by the platform tooling that we don't model yet.
    #[serde(untagged)]
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Device {
    /// Platform-specific identifier (adb serial, simulator UDID).
    pub id: String,
    pub platform: Platform,
    pub state: DeviceState,
    pub model: Option<String>,
    pub is_emulator: bool,
    /// The virtual device an emulator runs (stable across restarts, unlike its serial).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avd: Option<String>,
    /// OS API level, e.g. 36.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api: Option<u32>,
    /// `ro.product.manufacturer`, lowercase: `google`, `xiaomi`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
}

impl Device {
    /// `emulator-5554 (Pixel_9_Pro_XL, API 36)`, `R5CT… (SM-S918B, API 34)`.
    pub fn describe(&self) -> String {
        let name = self.avd.as_deref().or(self.model.as_deref());
        match (name, self.api) {
            (Some(n), Some(api)) => format!("{} ({n}, API {api})", self.id),
            (Some(n), None) => format!("{} ({n})", self.id),
            (None, Some(api)) => format!("{} (API {api})", self.id),
            (None, None) => self.id.clone(),
        }
    }
}

/// A virtual device that can be started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Avd {
    pub name: String,
    /// From its system image; `None` when the AVD's config can't be read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api: Option<u32>,
    /// The serial of the emulator running it, if one is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub running: Option<String>,
}

impl Avd {
    pub fn describe(&self) -> String {
        match self.api {
            Some(api) => format!("{} (API {api})", self.name),
            None => self.name.clone(),
        }
    }
}

/// A system appearance setting that UI checks vary, with its value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    /// The system font scale (`1.3`); `None` is the default.
    FontScale(Option<String>),
    /// Dark theme: `yes`, `no` or `auto`.
    NightMode(String),
    /// The app's own languages, comma-separated tags; empty follows the system.
    AppLocales { package: String, locales: String },
    /// Auto-rotate on or off, and the rotation used while it's off (0–3, quarter turns). Both are
    /// kept, so restoring a device puts back exactly what it had.
    Rotation { auto: bool, user: u32 },
    /// Display size (px) and density overrides; `None` is the panel's own.
    Display {
        size: Option<(u32, u32)>,
        density: Option<u32>,
    },
    /// The system time zone, an Olson id (`Asia/Tokyo`).
    TimeZone(String),
}

impl Appearance {
    /// Which setting this is, to read it before changing it.
    pub fn kind(&self) -> AppearanceKind {
        match self {
            Appearance::FontScale(_) => AppearanceKind::FontScale,
            Appearance::NightMode(_) => AppearanceKind::NightMode,
            Appearance::AppLocales { package, .. } => AppearanceKind::AppLocales {
                package: package.clone(),
            },
            Appearance::Rotation { .. } => AppearanceKind::Rotation,
            Appearance::Display { .. } => AppearanceKind::Display,
            Appearance::TimeZone(_) => AppearanceKind::TimeZone,
        }
    }
}

/// Which appearance setting to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppearanceKind {
    FontScale,
    NightMode,
    AppLocales { package: String },
    Rotation,
    Display,
    TimeZone,
}

/// The panel itself, whatever is overridden: size in px and density in dpi.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PhysicalDisplay {
    pub width: u32,
    pub height: u32,
    pub density: u32,
}
