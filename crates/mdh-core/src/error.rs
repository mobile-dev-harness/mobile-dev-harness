pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("`{name}` not found: {hint}")]
    ToolNotFound { name: String, hint: String },

    #[error("`{command}` failed (exit code {code:?}): {stderr}")]
    CommandFailed {
        command: String,
        code: Option<i32>,
        stderr: String,
    },

    #[error("unexpected output from `{tool}`: {detail}")]
    Parse { tool: String, detail: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
