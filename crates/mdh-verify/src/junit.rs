//! JUnit XML for CI (functional design F7.4): one test case per flow.

use crate::check::Outcome;
use crate::verdict::{Status, Verdict};

pub fn junit(suite: &str, verdicts: &[Verdict]) -> String {
    let failures = verdicts.iter().filter(|v| v.status == Status::Fail).count();
    let errors = verdicts
        .iter()
        .filter(|v| v.status == Status::Error)
        .count();
    let time: f64 = verdicts.iter().map(|v| v.duration_ms as f64 / 1000.0).sum();
    let mut out = vec![
        r#"<?xml version="1.0" encoding="UTF-8"?>"#.to_owned(),
        format!(
            r#"<testsuite name="{}" tests="{}" failures="{failures}" errors="{errors}" time="{time:.3}">"#,
            esc(suite),
            verdicts.len()
        ),
    ];
    for v in verdicts {
        let name = v.name.as_deref().unwrap_or("verify");
        out.push(format!(
            r#"  <testcase name="{}" classname="{}" time="{:.3}">"#,
            esc(name),
            esc(suite),
            v.duration_ms as f64 / 1000.0
        ));
        let failed: Vec<String> = v
            .findings
            .iter()
            .filter(|f| f.outcome >= Outcome::Fail)
            .map(|f| match &f.observed {
                Some(o) => format!("{} — {o}", f.check),
                None => f.check.clone(),
            })
            .collect();
        if let Some(first) = failed.first() {
            let tag = if v.status == Status::Error {
                "error"
            } else {
                "failure"
            };
            out.push(format!(
                r#"    <{tag} message="{}">{}</{tag}>"#,
                esc(first),
                esc(&v.text)
            ));
        }
        if let Some(dir) = &v.run_dir {
            out.push(format!(
                "    <system-out>evidence: {}</system-out>",
                esc(&dir.display().to_string())
            ));
        }
        out.push("  </testcase>".into());
    }
    out.push("</testsuite>".into());
    out.join("\n") + "\n"
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
