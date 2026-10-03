//! One verdict per verification: every finding of every check kind, and where the evidence is.

use std::path::PathBuf;

use mdh_core::Error;
use serde::Serialize;

use crate::check::{Finding, Outcome};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pass,
    Fail,
    /// A check couldn't be evaluated (an ambiguous target, a step that couldn't run).
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct Verdict {
    pub status: Status,
    /// The flow verified, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub findings: Vec<Finding>,
    pub duration_ms: u64,
    /// Where the evidence went: `.mdh/runs/<time>-verify/`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_dir: Option<PathBuf>,
    /// Evidence files in `run_dir`.
    pub evidence: Vec<String>,
    /// For flows: steps completed and steps in the flow.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<(usize, usize)>,
    /// The compact text form agents read.
    pub text: String,
}

impl Verdict {
    pub fn new(
        name: Option<String>,
        findings: Vec<Finding>,
        duration_ms: u64,
        run_dir: Option<PathBuf>,
        evidence: Vec<String>,
    ) -> Self {
        let worst = findings
            .iter()
            .map(|f| f.outcome)
            .max()
            .unwrap_or(Outcome::Pass);
        let status = match worst {
            Outcome::Error => Status::Error,
            Outcome::Fail => Status::Fail,
            Outcome::Pass | Outcome::Warn => Status::Pass,
        };
        let mut v = Verdict {
            status,
            name,
            findings,
            duration_ms,
            run_dir,
            evidence,
            steps: None,
            text: String::new(),
        };
        v.text = v.render();
        v
    }

    pub fn set_steps(&mut self, ran: usize, total: usize) {
        self.steps = Some((ran, total));
        self.text = self.render();
    }

    pub fn passed(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.outcome <= Outcome::Warn)
            .count()
    }

    /// `VERIFICATION_FAILED` unless everything passed; the verdict itself carries the details.
    pub fn failure(&self) -> Option<Error> {
        (self.status != Status::Pass).then(|| Error::VerificationFailed {
            failed: self.findings.len() - self.passed(),
            total: self.findings.len(),
        })
    }

    fn render(&self) -> String {
        let status = match self.status {
            Status::Pass => "PASS",
            Status::Fail => "FAIL",
            Status::Error => "ERROR",
        };
        let name = self
            .name
            .as_deref()
            .map(|n| format!(" {n}"))
            .unwrap_or_default();
        let steps = match self.steps {
            Some((1, 1)) => " · 1 step".to_owned(),
            Some((done, total)) if done == total => format!(" · {total} steps"),
            Some((done, total)) => format!(" · stopped after {done} of {total} steps"),
            None => String::new(),
        };
        let mut out = vec![format!(
            "verdict{name}: {status}{steps} · {} of {} checks passed · {:.1} s",
            self.passed(),
            self.findings.len(),
            self.duration_ms as f64 / 1000.0
        )];
        for f in &self.findings {
            let mark = match f.outcome {
                Outcome::Pass => "✓",
                Outcome::Warn => "!",
                Outcome::Fail => "✗",
                Outcome::Error => "?",
            };
            let step = f
                .step
                .map(|s| format!("step {}: ", s + 1))
                .unwrap_or_default();
            let mut line = format!("  {mark} {step}{}", f.check);
            if let Some(o) = &f.observed {
                line.push_str(&format!(" — {o}"));
            }
            out.push(line);
            for e in &f.evidence {
                out.extend(e.lines().map(|l| format!("    {l}")));
            }
        }
        if let Some(dir) = &self.run_dir {
            let files = if self.evidence.is_empty() {
                String::new()
            } else {
                format!(" ({})", self.evidence.join(", "))
            };
            out.push(format!("evidence: {}{files}", dir.display()));
        }
        out.join("\n")
    }
}
