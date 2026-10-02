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
    Io,
}

impl ErrorCode {
    /// Process exit code: 1 verification failed, 3 environment, 4 build, 5 app crashed, 10 internal.
    /// (2 is reserved for usage errors, which clap reports itself.)
    pub fn exit_code(self) -> u8 {
        match self {
            ErrorCode::ToolNotFound | ErrorCode::CommandFailed | ErrorCode::EnvironmentNotReady => {
                3
            }
            ErrorCode::UnexpectedOutput | ErrorCode::Io => 10,
        }
    }
}
