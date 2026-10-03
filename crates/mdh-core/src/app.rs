use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchInfo {
    /// The activity that ended up in front, e.g. `package/.Activity`.
    pub activity: Option<String>,
    /// Launch duration reported by the platform; 0 when nothing was started.
    pub total_time_ms: u64,
    /// Nothing was started: the intent went to an existing instance or task, so the app may still
    /// be on whatever screen it was on and navigation can't be assumed.
    pub reused_existing: bool,
    /// `COLD`, `WARM` or `HOT`, as the platform classified the launch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}
