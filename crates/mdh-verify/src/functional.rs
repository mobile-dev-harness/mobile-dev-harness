//! The built-in check kind: assertions on the screen and the logs.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use mdh_core::{LogLevel, Result};
use mdh_observe::{CrashKind, LogDigest, render_logs};

use crate::assertion::Assertion;
use crate::check::{Check, CheckContext, CheckKind, Finding, Outcome};

/// Between re-reads of the screen while checks don't hold yet.
const POLL: Duration = Duration::from_millis(250);
/// Log lines searched by `log` and `no log`.
const LOG_LINES: usize = 2000;

pub struct Functional {
    pub assertions: Vec<Assertion>,
    /// How long screen checks may take to start holding (a result that is still loading).
    pub timeout: Duration,
}

#[async_trait]
impl Check for Functional {
    fn kind(&self) -> CheckKind {
        CheckKind::Functional
    }

    async fn run(&self, cx: &mut CheckContext<'_>) -> Result<Vec<Finding>> {
        let screen: Vec<&Assertion> = self
            .assertions
            .iter()
            .filter(|a| a.is_screen_check())
            .collect();
        let started = Instant::now();
        let mut findings = Vec::new();
        if !screen.is_empty() {
            loop {
                let observed = cx.session.observe(false, cx.timings).await?;
                let activity = observed.screen.activity.as_deref();
                findings = screen
                    .iter()
                    .map(|a| a.evaluate(&observed.tree, activity))
                    .collect();
                let crashed = observed.logs.as_ref().is_some_and(|l| {
                    l.crashes
                        .iter()
                        .any(|c| c.of_app && c.kind != CrashKind::Died)
                });
                let holding = findings
                    .iter()
                    .all(|f: &Finding| f.outcome == Outcome::Pass);
                if holding || crashed || started.elapsed() >= self.timeout {
                    break;
                }
                tokio::time::sleep(POLL).await;
            }
        }
        for a in self.assertions.iter().filter(|a| !a.is_screen_check()) {
            findings.push(self.logs_check(a, cx).await?);
        }
        for f in &mut findings {
            f.step = cx.step;
        }
        Ok(findings)
    }
}

impl Functional {
    async fn logs_check(&self, a: &Assertion, cx: &mut CheckContext<'_>) -> Result<Finding> {
        let mut f = Finding {
            kind: CheckKind::Functional,
            outcome: Outcome::Pass,
            check: a.to_string(),
            observed: None,
            step: None,
            evidence: Vec::new(),
        };
        match a {
            Assertion::NoCrash => {
                let crashes = cx.session.app_crashes(cx.since_ms).await?;
                if let Some(first) = crashes.first() {
                    f.outcome = Outcome::Fail;
                    f.observed = Some(match crashes.len() {
                        1 => first.summary.clone(),
                        n => format!("{} (and {} more)", first.summary, n - 1),
                    });
                    f.evidence.push(render_logs(&LogDigest {
                        crashes,
                        ..LogDigest::default()
                    }));
                }
            }
            Assertion::Log { contains, level } | Assertion::NoLog { contains, level } => {
                let report = cx
                    .session
                    .logs(level.unwrap_or(LogLevel::Verbose), LOG_LINES)
                    .await?;
                let needle = contains.to_lowercase();
                let hit = report
                    .lines
                    .iter()
                    .rev()
                    .find(|l| l.to_lowercase().contains(&needle));
                let wanted = matches!(a, Assertion::Log { .. });
                match (hit, wanted) {
                    (Some(_), true) | (None, false) => {}
                    (None, true) => {
                        f.outcome = Outcome::Fail;
                        f.observed =
                            Some("no matching log line of the app in the last 10 minutes".into());
                    }
                    (Some(line), false) => {
                        f.outcome = Outcome::Fail;
                        f.observed = Some(line.trim().to_owned());
                    }
                }
            }
            _ => unreachable!("a screen check: {a}"),
        }
        Ok(f)
    }
}
