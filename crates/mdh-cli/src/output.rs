//! Renders command results as the ADR-0005 envelope (`--json`) or as compact text.

use std::process::ExitCode;
use std::time::Instant;

use mdh_core::Error;
use mdh_core::output::{Envelope, Timings, millis};
use serde::Serialize;

/// Human-readable rendering of a command's data.
pub trait Human {
    fn human(&self) -> String;
}

/// Prints the result and returns the process exit code derived from the error, if any.
pub fn finish<T: Serialize + Human>(
    json: bool,
    started: Instant,
    timings: Timings,
    data: Option<T>,
    error: Option<Error>,
) -> ExitCode {
    let exit = error.as_ref().map_or(0, |e| e.code().exit_code());

    if json {
        let mut envelope = Envelope::new(data, error.as_ref()).timing("total", millis(started));
        for (phase, ms) in timings.into_inner() {
            envelope = envelope.timing(phase, ms);
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&envelope).expect("envelope is always serializable")
        );
    } else {
        if let Some(data) = &data {
            println!("{}", data.human());
        }
        if let Some(e) = &error {
            eprintln!("error: {e}\nhint: {}", e.hint());
        }
    }

    ExitCode::from(exit)
}
