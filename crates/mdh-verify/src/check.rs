//! The interface every check kind implements (ADR-0009) and what checks report.

use std::path::Path;

use async_trait::async_trait;
use mdh_control::Session;
use mdh_core::Result;
use mdh_core::output::Timings;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckKind {
    Functional,
    Visual,
    Performance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Pass,
    /// Worth a look, not a failure.
    Warn,
    Fail,
    /// The check couldn't be evaluated: an invalid or ambiguous target, an unreadable screen.
    Error,
}

/// One checked expectation.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub kind: CheckKind,
    pub outcome: Outcome,
    /// What was checked, in the inline assertion syntax: `enabled id=sign_in`.
    pub check: String,
    /// What was observed, for failures and errors: `disabled: [e72] button "SIGN IN" disabled`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed: Option<String>,
    /// Which step of a flow it belongs to (0-based).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<usize>,
    /// Supporting excerpts: the crash report, the matching log line.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
}

/// What a check runs with: the session to drive and observe the app, where to put evidence, and
/// where in a flow it runs.
pub struct CheckContext<'a> {
    pub session: &'a mut Session,
    pub run_dir: Option<&'a Path>,
    pub step: Option<usize>,
    /// Device time the verification started; crashes and logs are looked for since then
    /// (`None`: since the session started).
    pub since_ms: Option<u64>,
    pub timings: &'a mut Timings,
}

#[async_trait]
pub trait Check: Send + Sync {
    fn kind(&self) -> CheckKind;
    async fn run(&self, cx: &mut CheckContext<'_>) -> Result<Vec<Finding>>;
}
