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

    #[error("no device online{}", if avds.is_empty() { String::new() } else { format!("; emulators that can be started: {}", avds.join(", ")) })]
    NoDevice {
        /// Startable virtual devices, `Pixel_9 (API 36)`.
        avds: Vec<String>,
    },

    #[error("emulator `{avd}` didn't start: {reason}")]
    EmulatorFailed { avd: String, reason: String },

    #[error("device `{id}` is not connected")]
    DeviceNotFound { id: String },

    #[error("several devices are online: {}", candidates.join(", "))]
    AmbiguousDevice { candidates: Vec<String> },

    #[error("on-device helper unavailable: {reason}")]
    HelperUnavailable { reason: String },

    #[error("could not install the on-device helper: {reason}{}", if detail.is_empty() { String::new() } else { format!(" ({detail})") })]
    HelperInstallFailed {
        /// Android's code, e.g. `INSTALL_FAILED_VERSION_DOWNGRADE`.
        reason: String,
        detail: String,
    },

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

    #[error(
        "`{target}` is covered by the app's own bars or by system windows (status bar, navigation bar, keyboard)"
    )]
    TargetObscured { target: String },

    #[error("no Gradle project in {dir} or its parents")]
    ProjectNotFound { dir: String },

    #[error("could not read the Gradle project: {message}")]
    ProbeFailed { message: String },

    #[error("several {what}s could be built: {}", candidates.join(", "))]
    AmbiguousBuildTarget {
        what: String,
        candidates: Vec<String>,
    },

    #[error("no {what} `{name}`; available: {}", candidates.join(", "))]
    UnknownBuildTarget {
        what: String,
        name: String,
        candidates: Vec<String>,
    },

    #[error("`{task}` failed with {errors} error(s)")]
    BuildFailed { task: String, errors: usize },

    #[error("no installable APK for `{variant}`: {reason}")]
    NoApk { variant: String, reason: String },

    #[error("install failed: {reason}{}", if detail.is_empty() { String::new() } else { format!(" ({detail})") })]
    InstallFailed {
        /// Android's code, e.g. `INSTALL_FAILED_UPDATE_INCOMPATIBLE`.
        reason: String,
        detail: String,
    },

    #[error("{operation} is not supported on this device")]
    Unsupported { operation: String },

    #[error("invalid check `{assertion}`: {reason}")]
    InvalidAssertion { assertion: String, reason: String },

    #[error("invalid flow `{flow}`: {reason}")]
    InvalidFlow { flow: String, reason: String },

    #[error("no flow `{name}`")]
    FlowNotFound {
        name: String,
        available: Vec<String>,
    },

    #[error("environment variable `{name}` is not set")]
    MissingSecret { name: String },

    #[error("verification failed: {failed} of {total} checks")]
    VerificationFailed { failed: usize, total: usize },

    /// A compatibility run in which risks failed; same code as a failed verification.
    #[error("compatibility: {failed} of {total} risks failed")]
    RisksFailed { failed: usize, total: usize },

    #[error("{dir} is not inside a git repository")]
    NotARepository { dir: String },

    #[error("unknown revision `{rev}`")]
    UnknownRevision { rev: String },

    /// Something the user has to agree to first (a download, say); never assumed.
    #[error("{action} needs the user's consent")]
    NeedsConsent { action: String, retry: String },

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
            Error::NoDevice { .. } | Error::DeviceNotFound { .. } => ErrorCode::DeviceNotFound,
            Error::EmulatorFailed { .. } => ErrorCode::EmulatorFailed,
            Error::AmbiguousDevice { .. } => ErrorCode::AmbiguousDevice,
            Error::HelperUnavailable { .. } | Error::HelperInstallFailed { .. } => {
                ErrorCode::HelperUnavailable
            }
            Error::HelperCommand { .. } => ErrorCode::HelperError,
            Error::AppNotFound { .. } => ErrorCode::AppNotFound,
            Error::LaunchFailed { .. } => ErrorCode::LaunchFailed,
            Error::ElementNotFound { .. } => ErrorCode::ElementNotFound,
            Error::AmbiguousTarget { .. } => ErrorCode::AmbiguousTarget,
            Error::InvalidTarget { .. } => ErrorCode::InvalidTarget,
            Error::Timeout { .. } => ErrorCode::Timeout,
            Error::AppCrashed { .. } => ErrorCode::AppCrashed,
            Error::TargetObscured { .. } => ErrorCode::TargetObscured,
            Error::ProjectNotFound { .. } => ErrorCode::ProjectNotFound,
            Error::ProbeFailed { .. } => ErrorCode::ProbeFailed,
            Error::AmbiguousBuildTarget { .. } => ErrorCode::AmbiguousBuildTarget,
            Error::UnknownBuildTarget { .. } => ErrorCode::UnknownBuildTarget,
            Error::BuildFailed { .. } => ErrorCode::BuildFailed,
            Error::NoApk { .. } => ErrorCode::NoApk,
            Error::InstallFailed { .. } => ErrorCode::InstallFailed,
            Error::Unsupported { .. } => ErrorCode::Unsupported,
            Error::InvalidAssertion { .. } => ErrorCode::InvalidAssertion,
            Error::VerificationFailed { .. } | Error::RisksFailed { .. } => {
                ErrorCode::VerificationFailed
            }
            Error::InvalidFlow { .. } => ErrorCode::InvalidFlow,
            Error::FlowNotFound { .. } => ErrorCode::FlowNotFound,
            Error::MissingSecret { .. } => ErrorCode::MissingSecret,
            Error::NotARepository { .. } => ErrorCode::NotARepository,
            Error::UnknownRevision { .. } => ErrorCode::UnknownRevision,
            Error::NeedsConsent { .. } => ErrorCode::NeedsConsent,
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
            Error::NoDevice { avds } if avds.is_empty() => {
                "connect a phone with USB debugging on, or create an emulator in Android Studio's Device \
                 Manager; then check `mdh devices`"
                    .into()
            }
            Error::NoDevice { .. } => {
                "ask the user whether to start one (it takes a while and uses memory), then \
                 `mdh emulator start <AVD>` (MCP: mdh_status with start_emulator)"
                    .into()
            }
            Error::DeviceNotFound { .. } => {
                "start an emulator or connect a device, then check `mdh devices`".into()
            }
            Error::EmulatorFailed { avd, .. } => format!(
                "start it from Android Studio's Device Manager or with `emulator -avd {avd}` to see what's wrong"
            ),
            Error::AmbiguousDevice { .. } => {
                "ask the user which one to use; pass `--device <id>`, or remember one for this project \
                 with `mdh devices use <id>` (MCP: mdh_status with device)"
                    .into()
            }
            Error::HelperUnavailable { .. } => {
                "only one UiAutomation client can run per device; stop uiautomator, Appium, Maestro or \
                 mobile-mcp sessions on it and retry"
                    .into()
            }
            Error::HelperInstallFailed { reason, .. } => helper_install_hint(reason).into(),
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
            Error::TargetObscured { .. } => {
                "scroll it into view or hide the keyboard (key BACK); if the app draws under the \
                 system bars, that is a layout bug worth fixing"
                    .into()
            }
            Error::ProjectNotFound { .. } => {
                "run from the project directory or pass --project <dir> (a directory with settings.gradle or settings.gradle.kts)".into()
            }
            Error::ProbeFailed { .. } => {
                "the Gradle build itself doesn't configure; run `./gradlew help` to see why".into()
            }
            Error::AmbiguousBuildTarget { what, .. } => format!("pick one with --{what}"),
            Error::UnknownBuildTarget { what, .. } => format!("pass one of the available {what}s with --{what}"),
            Error::BuildFailed { .. } => {
                "fix the diagnostics in the output; the full Gradle log is listed there too".into()
            }
            Error::NoApk { .. } => "build it first (run without --no-build), or build an APK for this device".into(),
            Error::InstallFailed { reason, .. } => install_hint(reason).into(),
            Error::Unsupported { .. } => "use an Android emulator or device".into(),
            Error::InvalidAssertion { .. } => {
                "write checks like `visible \"Sign in\"`, `enabled id=sign_in`, `text id=title == \"Inbox\"`, \
                 `screen .MainActivity`, `no crash` or `log ~= timeout`"
                    .into()
            }
            Error::InvalidFlow { .. } => {
                "fix the flow file; the format is in docs/design/01-functional.md §4.6".into()
            }
            Error::FlowNotFound { available, .. } if available.is_empty() => {
                "no flows saved yet; record one by acting on the app, then `mdh flow save <name>`".into()
            }
            Error::FlowNotFound { available, .. } => format!("saved flows: {}", available.join(", ")),
            Error::MissingSecret { name } => {
                format!("the flow types `${{env:{name}}}`; set {name} in the environment of mdh")
            }
            Error::RisksFailed { .. } => {
                "each failed risk says on which device or configuration and what broke; unverified risks say what's \
                 missing"
                    .into()
            }
            Error::VerificationFailed { .. } => {
                "the verdict shows what was observed for each failed check; evidence (screenshot, tree, \
                 logs) is in the run directory it names"
                    .into()
            }
            Error::NotARepository { .. } => {
                "impact analysis compares against git; run it inside the repository (or `git init` \
                 and commit a baseline first)"
                    .into()
            }
            Error::UnknownRevision { .. } => {
                "pass a branch, tag or commit that exists, e.g. `--base main` or `--base HEAD~1`".into()
            }
            Error::NeedsConsent { retry, .. } => {
                format!("ask the user; if they agree, {retry}")
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
    EmulatorFailed,
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
    TargetObscured,
    ProjectNotFound,
    ProbeFailed,
    AmbiguousBuildTarget,
    UnknownBuildTarget,
    BuildFailed,
    NoApk,
    InstallFailed,
    Unsupported,
    InvalidAssertion,
    VerificationFailed,
    InvalidFlow,
    FlowNotFound,
    MissingSecret,
    NotARepository,
    UnknownRevision,
    NeedsConsent,
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
            ErrorCode::EmulatorFailed => "EMULATOR_FAILED",
            ErrorCode::HelperUnavailable => "HELPER_UNAVAILABLE",
            ErrorCode::HelperError => "HELPER_ERROR",
            ErrorCode::AppNotFound => "APP_NOT_FOUND",
            ErrorCode::LaunchFailed => "LAUNCH_FAILED",
            ErrorCode::ElementNotFound => "ELEMENT_NOT_FOUND",
            ErrorCode::AmbiguousTarget => "AMBIGUOUS_TARGET",
            ErrorCode::InvalidTarget => "INVALID_TARGET",
            ErrorCode::Timeout => "TIMEOUT",
            ErrorCode::AppCrashed => "APP_CRASHED",
            ErrorCode::TargetObscured => "TARGET_OBSCURED",
            ErrorCode::ProjectNotFound => "PROJECT_NOT_FOUND",
            ErrorCode::ProbeFailed => "PROBE_FAILED",
            ErrorCode::AmbiguousBuildTarget => "AMBIGUOUS_BUILD_TARGET",
            ErrorCode::UnknownBuildTarget => "UNKNOWN_BUILD_TARGET",
            ErrorCode::BuildFailed => "BUILD_FAILED",
            ErrorCode::NoApk => "NO_APK",
            ErrorCode::InstallFailed => "INSTALL_FAILED",
            ErrorCode::Unsupported => "UNSUPPORTED",
            ErrorCode::InvalidAssertion => "INVALID_ASSERTION",
            ErrorCode::VerificationFailed => "VERIFICATION_FAILED",
            ErrorCode::InvalidFlow => "INVALID_FLOW",
            ErrorCode::FlowNotFound => "FLOW_NOT_FOUND",
            ErrorCode::MissingSecret => "MISSING_SECRET",
            ErrorCode::NotARepository => "NOT_A_REPOSITORY",
            ErrorCode::UnknownRevision => "UNKNOWN_REVISION",
            ErrorCode::NeedsConsent => "NEEDS_CONSENT",
            ErrorCode::Io => "IO",
        }
    }

    /// Process exit code: 1 the app didn't match expectations (verification failed, element missing,
    /// timeout), 2 usage, 3 environment, 4 build, 5 app crashed, 10 internal.
    pub fn exit_code(self) -> u8 {
        match self {
            ErrorCode::ElementNotFound
            | ErrorCode::VerificationFailed
            | ErrorCode::AmbiguousTarget
            | ErrorCode::Timeout
            | ErrorCode::TargetObscured => 1,
            ErrorCode::InvalidTarget
            | ErrorCode::InvalidAssertion
            | ErrorCode::InvalidFlow
            | ErrorCode::FlowNotFound
            | ErrorCode::MissingSecret
            | ErrorCode::AmbiguousBuildTarget
            | ErrorCode::UnknownBuildTarget
            | ErrorCode::UnknownRevision
            | ErrorCode::NeedsConsent => 2,
            ErrorCode::BuildFailed | ErrorCode::ProbeFailed | ErrorCode::NoApk => 4,
            ErrorCode::AppCrashed => 5,
            ErrorCode::ToolNotFound
            | ErrorCode::CommandFailed
            | ErrorCode::EnvironmentNotReady
            | ErrorCode::DeviceNotFound
            | ErrorCode::EmulatorFailed
            | ErrorCode::AmbiguousDevice
            | ErrorCode::HelperUnavailable
            | ErrorCode::AppNotFound
            | ErrorCode::LaunchFailed
            | ErrorCode::ProjectNotFound
            | ErrorCode::InstallFailed
            | ErrorCode::NotARepository
            | ErrorCode::Unsupported => 3,
            ErrorCode::UnexpectedOutput | ErrorCode::HelperError | ErrorCode::Io => 10,
        }
    }
}

/// What gets mdh's own helper installed after Android refused it.
fn helper_install_hint(reason: &str) -> &'static str {
    match reason {
        // mdh uninstalls a helper it can't replace in place, so here that didn't work either.
        "INSTALL_FAILED_VERSION_DOWNGRADE" | "INSTALL_FAILED_UPDATE_INCOMPATIBLE" => {
            "the helper already on the device (a newer one from another mdh, or one signed with another \
             key) could not be replaced; uninstall it by hand (`adb uninstall dev.mdh.helper`) and retry"
        }
        other => install_hint(other),
    }
}

/// What fixes Android's install failure codes (`pm` / `adb install`).
fn install_hint(reason: &str) -> &'static str {
    match reason {
        "INSTALL_FAILED_UPDATE_INCOMPATIBLE" | "INSTALL_FAILED_SHARED_USER_INCOMPATIBLE" => {
            "the installed app is signed with another key (built on another machine, or from a store); \
             uninstall it first: `mdh run --reinstall` does that (and clears the app's data)"
        }
        "INSTALL_FAILED_VERSION_DOWNGRADE" => {
            "a newer version is installed; `mdh run --reinstall` replaces it (and clears the app's data)"
        }
        "INSTALL_FAILED_NO_MATCHING_ABIS" => {
            "the APK has no native code for this device's CPU (`adb shell getprop ro.product.cpu.abilist`); \
             build that ABI or a universal APK"
        }
        "INSTALL_FAILED_OLDER_SDK" => {
            "the app's minSdk is higher than this device's Android version; use a newer emulator or device"
        }
        "INSTALL_FAILED_INSUFFICIENT_STORAGE" => {
            "the device is out of space; free some or wipe the emulator's data"
        }
        "INSTALL_FAILED_USER_RESTRICTED" | "INSTALL_FAILED_ABORTED" => {
            "the device blocked or asked to confirm the install; on Xiaomi, OPPO, vivo and similar ROMs \
             enable \"Install via USB\" in Developer options and accept the prompt on the device"
        }
        r if r.starts_with("INSTALL_PARSE_FAILED") => {
            "the APK is unsigned or damaged; build a debug variant or configure signing"
        }
        _ => "see Android's PackageManager failure codes for this reason",
    }
}
