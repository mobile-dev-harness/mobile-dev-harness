//! Risk: the compatibility risks of a mobile code change (ADR-0011), from what impact analysis
//! extracts and a knowledge base (`compat-kb`, vendored in `kb/`): OS versions, device types,
//! vendors and screen sizes, each with evidence and where it would show. No device: `mdh-compat`
//! plans and runs the verification.

pub mod kb;
pub mod render;
pub mod risk;

use std::path::Path;

use serde::Serialize;

pub use risk::{Dimension, Likelihood, Need, Risk, risks};

/// The change's compatibility risks.
#[derive(Debug, Clone, Serialize)]
pub struct RiskReport {
    pub base: String,
    pub risks: Vec<Risk>,
    pub text: String,
}

/// Risks of the change in `project` since `base`.
pub fn analyze(project: &Path, base: &str) -> mdh_impact::Result<RiskReport> {
    let report = mdh_impact::analyze(&mdh_impact::Options {
        project: project.to_owned(),
        base: base.to_owned(),
    })?;
    let risks = risks(&report);
    Ok(RiskReport {
        base: base.to_owned(),
        text: render::risks(base, &risks),
        risks,
    })
}

/// One line per risk for `mdh impact`'s compatibility section.
pub fn summaries(report: &mdh_impact::ImpactReport) -> Vec<String> {
    risks(report).iter().map(render::summary).collect()
}
