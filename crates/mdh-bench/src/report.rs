//! The results table: per setup, how often the agent was right, and what it cost.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::grade::Outcome;
use crate::setup::Setup;
use crate::task::{Kind, Level, Source};

fn first_prompt() -> u32 {
    1
}

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
    /// The prompt's answer format (`agent::PROMPT_VERSION`); version 1's results have none.
    #[serde(default = "first_prompt")]
    pub prompt: u32,
    #[serde(default)]
    pub app: Option<String>,
    #[serde(default)]
    pub level: Option<Level>,
    #[serde(default)]
    pub source: Option<Source>,
    /// The agent ran the app on the device after its last code change.
    #[serde(default)]
    pub checked: Option<bool>,
    /// Whether it works: the truth of a verify task, the hidden checks of a fix task (`None`: the
    /// grader couldn't tell). Version 1's results have none; `works` and the outcome tell.
    #[serde(default)]
    pub truth: Option<bool>,
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

impl Record {
    /// Whether what the agent judged or fixed works.
    fn works(&self) -> Option<bool> {
        if self.truth.is_some() {
            return self.truth;
        }
        match self.kind {
            Kind::Fix => self.works,
            Kind::Verify => match (self.outcome, self.answer) {
                (Outcome::Correct, a) => a,
                (Outcome::FalsePass, _) => Some(false),
                (Outcome::FalseFail, _) => Some(true),
                _ => None,
            },
        }
    }
}

/// The rates of a group of runs, grader errors left out.
struct Rates {
    runs: usize,
    correct: usize,
    false_pass: (usize, usize),
    precision: (usize, usize),
    false_fail: (usize, usize),
    abstained: usize,
    no_answer: usize,
    resolved: (usize, usize),
    unchecked: (usize, usize),
}

impl Rates {
    fn of(records: &[&Record]) -> Rates {
        let r: Vec<&&Record> = records
            .iter()
            .filter(|x| x.outcome != Outcome::GraderError)
            .collect();
        let count = |f: &dyn Fn(&Record) -> bool| r.iter().filter(|x| f(x)).count();
        let claims: Vec<&&&Record> = r.iter().filter(|x| x.answer == Some(true)).collect();
        let fixes = count(&|x| x.kind == Kind::Fix);
        Rates {
            runs: r.len(),
            correct: count(&|x| x.outcome == Outcome::Correct),
            false_pass: (
                count(&|x| x.outcome == Outcome::FalsePass),
                count(&|x| x.works() == Some(false)),
            ),
            precision: (
                claims.iter().filter(|x| x.works() == Some(true)).count(),
                claims.len(),
            ),
            false_fail: (
                count(&|x| x.outcome == Outcome::FalseFail),
                count(&|x| x.works() == Some(true)),
            ),
            abstained: count(&|x| x.outcome == Outcome::Abstained),
            no_answer: count(&|x| x.outcome == Outcome::NoAnswer),
            resolved: (
                count(&|x| x.kind == Kind::Fix && x.works == Some(true)),
                fixes,
            ),
            unchecked: (
                claims.iter().filter(|x| x.checked == Some(false)).count(),
                claims.iter().filter(|x| x.checked.is_some()).count(),
            ),
        }
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
        "| | Correct | False pass | Claim precision | False fail | Abstained | No answer | Resolved | Unchecked claims | Cost / run | Tokens / run | Tool calls | Screenshots | Time / run |".to_owned(),
        "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|".to_owned(),
    ]);
    let by_setup =
        |s: &Setup| -> Vec<&Record> { records.iter().filter(|r| r.setup == s.key()).collect() };
    for s in setups {
        let r = by_setup(s);
        if r.is_empty() {
            continue;
        }
        let rates = Rates::of(&r);
        let m = |f: &dyn Fn(&Record) -> f64| median(r.iter().map(|x| f(x)).collect());
        let ratio = |(n, d): (usize, usize)| pct(n, d);
        out.push(format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {:.0}k | {:.0} | {:.0} | {:.1} min |",
            s.name(),
            pct(rates.correct, rates.runs),
            ratio(rates.false_pass),
            ratio(rates.precision),
            ratio(rates.false_fail),
            pct(rates.abstained, rates.runs),
            pct(rates.no_answer, rates.runs),
            ratio(rates.resolved),
            ratio(rates.unchecked),
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
         (or is fixed), of the runs where it doesn't. Claim precision: of the PASS / FIXED answers, \
         those that are true. False fail: said it doesn't work, of the runs where it does. Abstained: \
         answered UNVERIFIED. Resolved: fix tasks whose hidden checks pass, whatever the agent said. \
         Unchecked claims: PASS / FIXED answers from runs that never ran the app after their last code \
         change. Grader errors are left out. Medians per run."
            .into(),
    );
    for (title, key) in [("Level", Group::Level), ("Source", Group::Source)] {
        if let Some(table) = grouped(records, setups, title, key) {
            out.push(String::new());
            out.push(table);
        }
    }
    let errors: Vec<String> = records
        .iter()
        .filter(|r| r.outcome == Outcome::GraderError)
        .map(|r| format!("- {} · {} · run {}: {}", r.task, r.setup, r.rep, r.detail))
        .collect();
    if !errors.is_empty() {
        out.push(String::new());
        out.push(format!("Grader errors, left out ({}):", errors.len()));
        out.extend(errors);
    }
    out.push(String::new());
    out.push(by_task(records, setups));
    out.join("\n")
}

#[derive(Clone, Copy)]
enum Group {
    Level,
    Source,
}

/// Correct and false-pass rates per level (or source) and setup; `None` when no run has one.
fn grouped(records: &[Record], setups: &[Setup], title: &str, key: Group) -> Option<String> {
    let label = |r: &Record| -> Option<String> {
        match key {
            Group::Level => r.level.map(|l| format!("{l:?}")),
            Group::Source => r.source.map(|s| format!("{s:?}").to_lowercase()),
        }
    };
    let mut groups: BTreeMap<String, Vec<&Record>> = BTreeMap::new();
    for r in records {
        if let Some(l) = label(r) {
            groups.entry(l).or_default().push(r);
        }
    }
    if groups.is_empty() {
        return None;
    }
    let shown: Vec<&Setup> = setups
        .iter()
        .filter(|s| records.iter().any(|r| r.setup == s.key()))
        .collect();
    let mut out = vec![
        format!(
            "| {title} (correct · false pass) | {} |",
            shown
                .iter()
                .map(|s| s.name())
                .collect::<Vec<_>>()
                .join(" | ")
        ),
        format!("|---|{}", "---|".repeat(shown.len())),
    ];
    for (group, rs) in &groups {
        let cells: Vec<String> = shown
            .iter()
            .map(|s| {
                let r: Vec<&Record> = rs.iter().copied().filter(|r| r.setup == s.key()).collect();
                let rates = Rates::of(&r);
                format!(
                    "{} · {}",
                    pct(rates.correct, rates.runs),
                    pct(rates.false_pass.0, rates.false_pass.1)
                )
            })
            .collect();
        out.push(format!("| {group} | {} |", cells.join(" | ")));
    }
    Some(out.join("\n"))
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
    out.push(
        "✓ correct · ✗ false pass · ⊘ false fail · – not fixed, said so · ~ unverified · ? no answer \
         · ! grader error"
            .into(),
    );
    out.join("\n")
}

fn mark(o: Outcome) -> &'static str {
    match o {
        Outcome::Correct => "✓",
        Outcome::FalsePass => "✗",
        Outcome::FalseFail => "⊘",
        Outcome::HonestFail => "–",
        Outcome::Abstained => "~",
        Outcome::NoAnswer => "?",
        Outcome::GraderError => "!",
    }
}
