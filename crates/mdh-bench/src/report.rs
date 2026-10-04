//! The results table: per setup, how often the agent was right, and what it cost.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::grade::Outcome;
use crate::setup::Setup;
use crate::task::Kind;

/// One run, as `results.jsonl` keeps it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub task: String,
    pub kind: Kind,
    pub setup: String,
    pub rep: usize,
    /// The model, and the provider when it isn't Anthropic.
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub provider: Option<String>,
    /// The device: AVD, API level, display size and density.
    #[serde(default)]
    pub device: Option<String>,
    pub outcome: Outcome,
    /// The agent's answer: PASS / FIXED is true.
    pub answer: Option<bool>,
    /// Fix tasks: the hidden checks passed.
    pub works: Option<bool>,
    pub detail: String,
    /// Verify tasks: the agent edited code it was told to leave alone.
    pub edited: bool,
    pub cost_usd: f64,
    pub tokens: u64,
    pub output_tokens: u64,
    pub tool_calls: u64,
    pub images: u64,
    pub turns: u64,
    pub duration_ms: u64,
    pub timed_out: bool,
    pub error: Option<String>,
}

fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    let m = v.len() / 2;
    if v.len() % 2 == 1 {
        v[m]
    } else {
        (v[m - 1] + v[m]) / 2.0
    }
}

fn pct(n: usize, d: usize) -> String {
    if d == 0 {
        "–".into()
    } else {
        format!("{:.0}% ({n}/{d})", n as f64 * 100.0 / d as f64)
    }
}

pub fn markdown(records: &[Record], setups: &[Setup]) -> String {
    let mut models: Vec<String> = records
        .iter()
        .map(|r| match &r.provider {
            Some(p) => format!("{} ({p})", r.model),
            None if r.model.is_empty() => "claude-sonnet-5-5".to_owned(),
            None => r.model.clone(),
        })
        .collect();
    models.sort();
    models.dedup();
    // Claude Code prices other providers' tokens as if they were Anthropic's, or not at all.
    let priced = records.iter().all(|r| r.provider.is_none());
    let mut devices: Vec<&str> = records.iter().filter_map(|r| r.device.as_deref()).collect();
    devices.sort_unstable();
    devices.dedup();
    let mut out = vec![format!("Model: {}", models.join(", "))];
    if !devices.is_empty() {
        out.push(format!("Device: {}", devices.join(", ")));
    }
    out.extend([
        String::new(),
        "| | Correct | False pass | False fail | Fixed | No answer | Cost / run | Tokens / run | Tool calls | Screenshots | Time / run |".to_owned(),
        "|---|---|---|---|---|---|---|---|---|---|---|".to_owned(),
    ]);
    for s in setups {
        let r: Vec<&Record> = records.iter().filter(|r| r.setup == s.key()).collect();
        if r.is_empty() {
            continue;
        }
        let count = |o: Outcome| r.iter().filter(|x| x.outcome == o).count();
        // A false pass can only happen where it doesn't work; a false fail where it does.
        let broken = r
            .iter()
            .filter(|x| match x.kind {
                Kind::Verify => {
                    x.outcome == Outcome::FalsePass
                        || (x.outcome == Outcome::Correct && x.answer == Some(false))
                }
                Kind::Fix => x.works == Some(false),
            })
            .count();
        let working = r
            .iter()
            .filter(|x| match x.kind {
                Kind::Verify => {
                    x.outcome == Outcome::FalseFail
                        || (x.outcome == Outcome::Correct && x.answer == Some(true))
                }
                Kind::Fix => x.works == Some(true),
            })
            .count();
        let m = |f: &dyn Fn(&Record) -> f64| median(r.iter().map(|x| f(x)).collect());
        let fixes: Vec<&&Record> = r.iter().filter(|x| x.kind == Kind::Fix).collect();
        let fixed = fixes.iter().filter(|x| x.works == Some(true)).count();
        out.push(format!(
            "| {} | {} | {} | {} | {} | {} | {} | {:.0}k | {:.0} | {:.0} | {:.1} min |",
            s.name(),
            pct(count(Outcome::Correct), r.len()),
            pct(count(Outcome::FalsePass), broken),
            pct(count(Outcome::FalseFail), working),
            pct(fixed, fixes.len()),
            pct(count(Outcome::NoAnswer), r.len()),
            if priced {
                format!("${:.2}", m(&|x| x.cost_usd))
            } else {
                "–".into()
            },
            m(&|x| x.tokens as f64) / 1000.0,
            m(&|x| x.tool_calls as f64),
            m(&|x| x.images as f64),
            m(&|x| x.duration_ms as f64) / 60_000.0,
        ));
    }
    out.push(String::new());
    out.push(
        "Correct: verify tasks judged right, fix tasks fixed and said so. False pass: said it works \
         (or is fixed) when it doesn't, of the runs where it doesn't. False fail: said it doesn't when \
         it does, of the runs where it does (an agent without a device that fixed the bug but couldn't \
         check it lands here). Fixed: fix tasks whose hidden checks pass, whatever the agent said. \
         Medians per run."
            .into(),
    );
    out.push(String::new());
    out.push(by_task(records, setups));
    out.join("\n")
}

/// Per task and setup: the outcomes of its runs.
fn by_task(records: &[Record], setups: &[Setup]) -> String {
    let mut tasks: BTreeMap<&str, BTreeMap<&str, Vec<&Record>>> = BTreeMap::new();
    for r in records {
        tasks
            .entry(&r.task)
            .or_default()
            .entry(&r.setup)
            .or_default()
            .push(r);
    }
    let shown: Vec<&Setup> = setups
        .iter()
        .filter(|s| records.iter().any(|r| r.setup == s.key()))
        .collect();
    let mut out = vec![
        format!(
            "| Task | {} |",
            shown
                .iter()
                .map(|s| s.name())
                .collect::<Vec<_>>()
                .join(" | ")
        ),
        format!("|---|{}", "---|".repeat(shown.len())),
    ];
    for (task, by_setup) in &tasks {
        let cells: Vec<String> = shown
            .iter()
            .map(|s| {
                by_setup
                    .get(s.key())
                    .map(|rs| {
                        rs.iter()
                            .map(|r| mark(r.outcome))
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .unwrap_or_default()
            })
            .collect();
        out.push(format!("| {task} | {} |", cells.join(" | ")));
    }
    out.push(String::new());
    out.push("✓ correct · ✗ false pass · ⊘ false fail · – not fixed, said so · ? no answer".into());
    out.join("\n")
}

fn mark(o: Outcome) -> &'static str {
    match o {
        Outcome::Correct => "✓",
        Outcome::FalsePass => "✗",
        Outcome::FalseFail => "⊘",
        Outcome::HonestFail => "–",
        Outcome::NoAnswer => "?",
    }
}
