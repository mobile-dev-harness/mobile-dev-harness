//! Text for agents: plans and run results, one line per risk plus its evidence.

use mdh_risk::kb::Shape;
use mdh_risk::render::{evidence, header};
use mdh_risk::risk::Risk;

use crate::plan::Plan;
use crate::run::{CompatReport, RiskStatus};

/// `mdh compat plan`: the cells, what they cost, what can't be covered.
pub fn plan(risks: &[Risk], plan: &Plan) -> String {
    let mut out = vec![format!(
        "compat plan: {} risk{}, {} cell{}",
        risks.len(),
        if risks.len() == 1 { "" } else { "s" },
        plan.cells.len(),
        if plan.cells.len() == 1 { "" } else { "s" }
    )];
    for c in &plan.cells {
        let what = if c.reference {
            "reference".to_owned()
        } else {
            c.risks.join(", ")
        };
        let cost = match (&c.target, c.shape) {
            (crate::plan::Target::Start { .. }, _) => " — boots an emulator (asks first)",
            (_, Shape::Default) => "",
            _ => " — display override, restored after",
        };
        out.push(format!("  {}: {what}{cost}", c.describe()));
    }
    for (id, why) in &plan.unverifiable {
        out.push(format!("  ? {id}: {why}"));
    }
    out.join("\n")
}

/// `mdh compat run`: a verdict per risk.
pub fn run(r: &CompatReport) -> String {
    let count = |s: RiskStatus| r.risks.iter().filter(|x| x.status == s).count();
    let mut out = vec![format!(
        "compat: {} risk{} · {} failed · {} unverified · {} passed · {} cell{} · {:.0} s",
        r.risks.len(),
        if r.risks.len() == 1 { "" } else { "s" },
        count(RiskStatus::Failed),
        count(RiskStatus::Unverified),
        count(RiskStatus::Passed),
        r.plan.cells.len(),
        if r.plan.cells.len() == 1 { "" } else { "s" },
        r.duration_ms as f64 / 1000.0
    )];
    for x in &r.risks {
        let mark = match x.status {
            RiskStatus::Failed => "✗",
            RiskStatus::Unverified => "?",
            RiskStatus::Passed => "✓",
        };
        out.push(format!("{mark} {}", header(&x.risk)));
        evidence(&x.risk, &mut out);
        for n in &x.notes {
            out.push(format!("    {n}"));
        }
        if x.status == RiskStatus::Unverified {
            out.push(format!("    check by hand: {}", x.risk.verify));
        }
    }
    out.join("\n")
}
