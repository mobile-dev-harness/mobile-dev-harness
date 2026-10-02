use serde::Serialize;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("`{name}` not found")]
    ToolNotFound { name: String, hint: String },

    #[error("`{command}` failed (exit code {code:?}): {stderr}")]
    CommandFailed {
        command: String,
        code: Option<i32>,
        stderr: String,
    },

    #[error("unexpected output from `{tool}`: {detail}")]
    Parse { tool: String, detail: String },

    #[error("environment not ready: {}", failed.join(", "))]
    EnvironmentNotReady { failed: Vec<String> },

    #[error("no online device")]
    NoDevice,

    #[error("device `{id}` is not connected")]
    DeviceNotFound { id: String },

    #[error("several devices are online: {}", candidates.join(", "))]
    AmbiguousDevice { candidates: Vec<String> },

    #[error("on-device helper unavailable: {reason}")]
    HelperUnavailable { reason: String },

    #[error("helper command `{cmd}` failed: {message}")]
    HelperCommand { cmd: String, message: String },

    #[error("app `{package}` is not installed or has no launcher activity")]
    AppNotFound { package: String },

    #[error("could not launch `{target}`: {message}")]
    LaunchFailed { target: String, message: String },

    #[error("no element matches `{target}`")]
    ElementNotFound {
        target: String,
        /// Rendered lines of the closest elements on screen.
        candidates: Vec<String>,
    },

    #[error("`{target}` matches {} elements", candidates.len())]
    AmbiguousTarget {
        target: String,
        candidates: Vec<String>,
    },

    #[error("invalid target `{target}`: {reason}")]
    InvalidTarget { target: String, reason: String },

    #[error("timed out after {seconds}s waiting for {what}")]
    Timeout { what: String, seconds: u64 },

    #[error("{package} crashed: {summary}")]
    AppCrashed { package: String, summary: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl Error {
    pub fn code(&self) -> ErrorCode {
        match self {
            Error::ToolNotFound { .. } => ErrorCode::ToolNotFound,
            Error::CommandFailed { .. } => ErrorCode::CommandFailed,
            Error::Parse { .. } => ErrorCode::UnexpectedOutput,
            Error::EnvironmentNotReady { .. } => ErrorCode::EnvironmentNotReady,
            Error::NoDevice | Error::DeviceNotFound { .. } => ErrorCode::DeviceNotFound,
            Error::AmbiguousDevice { .. } => ErrorCode::AmbiguousDevice,
            Error::HelperUnavailable { .. } => ErrorCode::HelperUnavailable,
            Error::HelperCommand { .. } => ErrorCode::HelperError,
            Error::AppNotFound { .. } => ErrorCode::AppNotFound,
            Error::LaunchFailed { .. } => ErrorCode::LaunchFailed,
            Error::ElementNotFound { .. } => ErrorCode::ElementNotFound,
            Error::AmbiguousTarget { .. } => ErrorCode::AmbiguousTarget,
            Error::InvalidTarget { .. } => ErrorCode::InvalidTarget,
            Error::Timeout { .. } => ErrorCode::Timeout,
            Error::AppCrashed { .. } => ErrorCode::AppCrashed,
            Error::Io(_) => ErrorCode::Io,
        }
    }

    /// What the caller should do next. Every error has one (ADR-0005).
    pub fn hint(&self) -> String {
        match self {
            Error::ToolNotFound { hint, .. } => hint.clone(),
            Error::CommandFailed { .. } => {
                "check that the device is connected (`mdh devices`); rerun with -v for details".into()
            }
            Error::Parse { .. } => {
                "this tool version may be unsupported; please report it with the output of `mdh doctor --json`"
                    .into()
            }
            Error::EnvironmentNotReady { .. } => "fix the failing checks, then rerun `mdh doctor`".into(),
            Error::NoDevice | Error::DeviceNotFound { .. } => {
                "start an emulator or connect a device, then check `mdh devices`".into()
            }
            Error::AmbiguousDevice { .. } => "pick one with `--device <id>`".into(),
            Error::HelperUnavailable { .. } => {
                "only one UiAutomation client can run per device; stop uiautomator, Appium, Maestro or \
                 mobile-mcp sessions on it and retry"
                    .into()
            }
            Error::HelperCommand { .. } => "check the device screen state and retry".into(),
            Error::AppNotFound { .. } => {
                "check the package name (`adb shell pm list packages`) or install the app first".into()
            }
            Error::LaunchFailed { .. } => {
                "check the component name; activities started directly must be exported".into()
            }
            Error::ElementNotFound { candidates, .. } if candidates.is_empty() => {
                "observe the screen to see what is there".into()
            }
            Error::ElementNotFound { candidates, .. } => {
                format!("closest matches: {}", candidates.join("; "))
            }
            Error::AmbiguousTarget { candidates, .. } => format!(
                "use a ref or a more specific selector; matches: {}",
                candidates.join("; ")
            ),
            Error::InvalidTarget { .. } => {
                "use a ref (`e12`), coordinates (`100,200`), a selector (`id=…`, `text=…`, `text~=…`, \
                 `role=…`, combined with `;`) or a label"
                    .into()
            }
            Error::Timeout { .. } => {
                "the UI didn't reach the expected state; observe to see where it is".into()
            }
            Error::AppCrashed { .. } => {
                "the crash report is in the output; fix the cause, then relaunch the app".into()
            }
            Error::Io(_) => "check that the path exists and is accessible".into(),
        }
    }
}

/// Stable, machine-checkable error identifiers. Renaming a variant is a breaking change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[non_exhaustive]
pub enum ErrorCode {
    ToolNotFound,
    CommandFailed,
    UnexpectedOutput,
    EnvironmentNotReady,
    DeviceNotFound,
    AmbiguousDevice,
    HelperUnavailable,
    HelperError,
    AppNotFound,
    LaunchFailed,
    ElementNotFound,
    AmbiguousTarget,
    InvalidTarget,
    Timeout,
    AppCrashed,
    Io,
}

impl ErrorCode {
    /// The stable wire form, identical to the serialized value.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::ToolNotFound => "TOOL_NOT_FOUND",
            ErrorCode::CommandFailed => "COMMAND_FAILED",
            ErrorCode::UnexpectedOutput => "UNEXPECTED_OUTPUT",
            ErrorCode::EnvironmentNotReady => "ENVIRONMENT_NOT_READY",
            ErrorCode::DeviceNotFound => "DEVICE_NOT_FOUND",
            ErrorCode::AmbiguousDevice => "AMBIGUOUS_DEVICE",
            ErrorCode::HelperUnavailable => "HELPER_UNAVAILABLE",
            ErrorCode::HelperError => "HELPER_ERROR",
            ErrorCode::AppNotFound => "APP_NOT_FOUND",
            ErrorCode::LaunchFailed => "LAUNCH_FAILED",
            ErrorCode::ElementNotFound => "ELEMENT_NOT_FOUND",
            ErrorCode::AmbiguousTarget => "AMBIGUOUS_TARGET",
            ErrorCode::InvalidTarget => "INVALID_TARGET",
            ErrorCode::Timeout => "TIMEOUT",
            ErrorCode::AppCrashed => "APP_CRASHED",
            ErrorCode::Io => "IO",
        }
    }

    /// Process exit code: 1 the app didn't match expectations (verification failed, element missing,
    /// timeout), 2 usage, 3 environment, 4 build, 5 app crashed, 10 internal.
    pub fn exit_code(self) -> u8 {
        match self {
            ErrorCode::ElementNotFound | ErrorCode::AmbiguousTarget | ErrorCode::Timeout => 1,
            ErrorCode::InvalidTarget => 2,
            ErrorCode::AppCrashed => 5,
            ErrorCode::ToolNotFound
            | ErrorCode::CommandFailed
            | ErrorCode::EnvironmentNotReady
            | ErrorCode::DeviceNotFound
            | ErrorCode::AmbiguousDevice
            | ErrorCode::HelperUnavailable
            | ErrorCode::AppNotFound
            | ErrorCode::LaunchFailed => 3,
            ErrorCode::UnexpectedOutput | ErrorCode::HelperError | ErrorCode::Io => 10,
        }
    }
}
