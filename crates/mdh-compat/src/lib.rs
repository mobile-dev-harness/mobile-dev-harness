//! Compat: compatibility, verified risk by risk (ADR-0011). Not a check kind and not a matrix
//! run for its own sake: impact analysis says what changed, [`risk`] what that puts at risk on
//! other OS versions, device types, vendors and screen sizes (with the knowledge base in [`kb`]),
//! [`plan`] the fewest cells that would show it, and the run verifies each risk there.
//!
//! Scope (functional design F12; milestone M7).

pub mod kb;
pub mod plan;
pub mod render;
pub mod risk;
pub mod run;

pub use plan::{Cell, Inventory, Plan, PlanOptions, Target, plan};
pub use risk::{Dimension, Likelihood, Need, Risk, risks};
pub use run::{CompatOptions, CompatReport, RiskStatus, inventory, prepare, run};

use std::path::Path;

use mdh_control::Session;
use mdh_core::Result;
use serde::Serialize;

/// The change's compatibility risks; no device needed.
#[derive(Debug, Clone, Serialize)]
pub struct RiskReport {
    pub base: String,
    pub risks: Vec<Risk>,
    pub text: String,
}

/// Risks of the change in `project` since `base`.
pub fn analyze(project: &Path, base: &str) -> Result<RiskReport> {
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

/// The risks and the cells that would verify them on what is connected.
#[derive(Debug, Clone, Serialize)]
pub struct PlanReport {
    pub risks: Vec<Risk>,
    pub plan: Plan,
    pub text: String,
}

pub async fn plan_for(session: &Session, options: &CompatOptions) -> Result<PlanReport> {
    let (_, risks, plan) = prepare(session, options).await?;
    Ok(PlanReport {
        text: render::plan(&risks, &plan),
        risks,
        plan,
    })
}

/// One line per risk for `mdh impact`'s compatibility section.
pub fn summaries(report: &mdh_impact::ImpactReport) -> Vec<String> {
    risks(report).iter().map(render::summary).collect()
}
