use std::path::Path;
use std::time::Duration;

use mdh_core::{Error, Result};
use tokio::process::Command;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// Runs `program` to completion and returns its stdout, or `CommandFailed` with stderr.
pub(crate) async fn run(program: &Path, args: &[&str]) -> Result<String> {
    run_with_timeout(program, args, DEFAULT_TIMEOUT).await
}

/// Runs `program` and returns stdout and stderr regardless of the exit status, for tools that
/// report errors on stdout (e.g. `am start`).
pub(crate) async fn run_capture(
    program: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<(String, String)> {
    let child = Command::new(program).args(args).kill_on_drop(true).output();
    let output =
        tokio::time::timeout(timeout, child)
            .await
            .map_err(|_| Error::CommandFailed {
                command: format!("{} {}", program.display(), args.join(" ")),
                code: None,
                stderr: format!("timed out after {}s", timeout.as_secs()),
            })??;
    Ok((
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

/// Like [`run`], killing the process if it outlives `timeout`.
pub(crate) async fn run_with_timeout(
    program: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<String> {
    let command_line = || format!("{} {}", program.display(), args.join(" "));
    let child = Command::new(program).args(args).kill_on_drop(true).output();
    let output =
        tokio::time::timeout(timeout, child)
            .await
            .map_err(|_| Error::CommandFailed {
                command: command_line(),
                code: None,
                stderr: format!("timed out after {}s", timeout.as_secs()),
            })??;
    if !output.status.success() {
        return Err(Error::CommandFailed {
            command: command_line(),
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Like [`run_with_timeout`] for binary stdout.
pub(crate) async fn run_bytes(program: &Path, args: &[&str], timeout: Duration) -> Result<Vec<u8>> {
    let command_line = || format!("{} {}", program.display(), args.join(" "));
    let child = Command::new(program).args(args).kill_on_drop(true).output();
    let output =
        tokio::time::timeout(timeout, child)
            .await
            .map_err(|_| Error::CommandFailed {
                command: command_line(),
                code: None,
                stderr: format!("timed out after {}s", timeout.as_secs()),
            })??;
    if !output.status.success() {
        return Err(Error::CommandFailed {
            command: command_line(),
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    Ok(output.stdout)
}
