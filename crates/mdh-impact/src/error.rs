//! What can go wrong reading a project's history. Impact analysis needs no device and no other mdh
//! crate; callers that report errors in mdh's envelope convert these (`mdh_verify::impact_error`).

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{dir} is not inside a git repository")]
    NotARepository { dir: String },

    #[error("unknown revision `{rev}`")]
    UnknownRevision { rev: String },

    #[error("no Gradle project in {dir} or its parents")]
    ProjectNotFound { dir: String },

    /// git isn't installed.
    #[error("`git` not found")]
    GitNotFound,

    #[error("`{command}` failed (exit code {code:?}): {stderr}")]
    Git {
        command: String,
        code: Option<i32>,
        stderr: String,
    },

    #[error("unexpected output from `git cat-file`: {detail}")]
    Parse { detail: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
