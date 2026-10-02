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
}
