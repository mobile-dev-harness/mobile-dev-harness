//! Text for agents: one line per risk, its evidence, and where it would show.

use crate::risk::{Likelihood, Risk};

/// Evidence lines shown per risk.
const EVIDENCE: usize = 2;

pub fn likelihood(l: Likelihood) -> &'static str {
    match l {
        Likelihood::High => "high",
        Likelihood::Medium => "medium",
        Likelihood::Low => "low",
    }
}

pub fn header(r: &Risk) -> String {
    format!(
        "{} · {} · {}",
        r.dimension.name(),
        likelihood(r.likelihood),
        r.title
    )
}

pub fn evidence(r: &Risk, out: &mut Vec<String>) {
    for e in r.evidence.iter().take(EVIDENCE) {
        out.push(format!("    {e}"));
    }
    if r.evidence.len() > EVIDENCE {
        out.push(format!("    … {} more", r.evidence.len() - EVIDENCE));
    }
}

pub fn where_(r: &Risk) -> String {
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
