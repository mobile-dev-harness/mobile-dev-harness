//! Human-readable forms of library results. JSON output serializes the same values.

use mdh_control::{
    ActOutcome, LogsReport, Observation, RunReport, Screenshot, SessionSummary, launch_text,
};
use mdh_core::LaunchInfo;
use serde::Serialize;

use crate::output::Human;

/// Data of commands that only report success.
#[derive(Serialize)]
pub struct Done {
    pub done: String,
}

impl Done {
    pub fn new(done: impl Into<String>) -> Self {
        Self { done: done.into() }
    }
}

impl Human for Done {
    fn human(&self) -> String {
        self.done.clone()
    }
}

impl Human for Observation {
    fn human(&self) -> String {
        self.text.clone()
    }
}

impl Human for ActOutcome {
    fn human(&self) -> String {
        self.text.clone()
    }
}

impl Human for RunReport {
    fn human(&self) -> String {
        self.text.clone()
    }
}

impl Human for Screenshot {
    fn human(&self) -> String {
        self.text()
    }
}

impl Human for LaunchInfo {
    fn human(&self) -> String {
        launch_text(self)
    }
}

impl Human for LogsReport {
    fn human(&self) -> String {
        self.text()
    }
}

impl Human for SessionSummary {
    fn human(&self) -> String {
        self.text()
    }
}

impl Human for mdh_impact::ImpactReport {
    fn human(&self) -> String {
        mdh_impact::render(self)
    }
}

impl Human for mdh_compat::RiskReport {
    fn human(&self) -> String {
        self.text.clone()
    }
}

impl Human for mdh_compat::PlanReport {
    fn human(&self) -> String {
        self.text.clone()
    }
}

impl Human for mdh_compat::CompatReport {
    fn human(&self) -> String {
        self.text.clone()
    }
}

impl Human for mdh_verify::Verdict {
    fn human(&self) -> String {
        self.text.clone()
    }
}

impl Human for mdh_verify::FlowRuns {
    fn human(&self) -> String {
        self.text.clone()
    }
}
