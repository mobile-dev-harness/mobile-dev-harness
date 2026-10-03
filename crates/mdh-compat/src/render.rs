//! Text for agents: risks, plans and run results, one line per risk plus its evidence.

use crate::kb::Shape;
use crate::plan::Plan;
use crate::risk::{Likelihood, Risk};
use crate::run::{CompatReport, RiskStatus};

/// Evidence lines shown per risk.
const EVIDENCE: usize = 2;

fn likelihood(l: Likelihood) -> &'static str {
    match l {
        Likelihood::High => "high",
        Likelihood::Medium => "medium",
        Likelihood::Low => "low",
    }
}

fn header(r: &Risk) -> String {
    format!(
        "{} · {} · {}",
        r.dimension.name(),
        likelihood(r.likelihood),
        r.title
    )
}

fn evidence(r: &Risk, out: &mut Vec<String>) {
    for e in r.evidence.iter().take(EVIDENCE) {
        out.push(format!("    {e}"));
    }
    if r.evidence.len() > EVIDENCE {
        out.push(format!("    … {} more", r.evidence.len() - EVIDENCE));
    }
}

fn where_(r: &Risk) -> String {
    if let Some(u) = &r.unverifiable {
        return u.clone();
    }
    let needs: Vec<String> = r.needs.iter().map(|n| n.describe()).collect();
    let mut s = format!("verify on {}", needs.join(" and "));
    if r.state_check {
        s.push_str(", state across rotation");
    }
    if !r.screens.is_empty() {
        s.push_str(&format!(" · {}", r.screens.join(", ")));
    } else if r.all_flows {
        s.push_str(" · every flow");
    }
    s
}

/// `screen size: layout at other screen sizes and orientations — verify on compact 360×640 dp
/// and tablet 1280×800 dp`.
pub fn summary(r: &Risk) -> String {
    format!("{}: {} — {}", r.dimension.name(), r.title, where_(r))
}

/// `mdh compat risks`: what the change puts at risk, and where it would show.
pub fn risks(base: &str, risks: &[Risk]) -> String {
    if risks.is_empty() {
        return format!("compat: no compatibility risks in the change since {base}");
    }
    let mut out = vec![format!(
        "compat: {} risk{} in the change since {base}",
        risks.len(),
        if risks.len() == 1 { "" } else { "s" }
    )];
    for r in risks {
        out.push(format!("- {}", header(r)));
        evidence(r, &mut out);
        out.push(format!("    {}", where_(r)));
        out.push(format!("    check: {}", r.verify));
    }
    out.join("\n")
}

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
