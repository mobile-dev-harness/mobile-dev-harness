use std::path::Path;

use mdh_core::{Error, Result};
use tokio::process::Command;

/// Runs `program` to completion and returns its stdout, or `CommandFailed` with stderr.
pub(crate) async fn run(program: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new(program).args(args).output().await?;
    if !output.status.success() {
        return Err(Error::CommandFailed {
            command: format!("{} {}", program.display(), args.join(" ")),
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
