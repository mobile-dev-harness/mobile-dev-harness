//! Agent integration (functional design F9.2, F9.3): project setup, the instructions agents read,
//! and the checks behind the Claude Code plugin's hooks.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use mdh_core::Result;

const BEGIN: &str = "<!-- mdh:begin -->";
const END: &str = "<!-- mdh:end -->";

/// The "how to verify" section `mdh init` keeps in `AGENTS.md` (Codex, Cursor and other agents read
/// it; the Claude Code plugin brings the same protocol as a skill).
pub const AGENTS_SECTION: &str = r#"## Verifying changes (mobile-dev-harness)

This app is verified with [mobile-dev-harness](https://github.com/qkmaosjtu/mobile-dev-harness)
(`mdh`). A change is done when it has a passing verdict on a device or emulator, not when it
compiles.

1. `mdh impact` — which screens the change reaches, how to get there and what to check (no device).
2. `mdh run` — build, install and start the app; build errors come back as `file:line`.
3. Drive each affected screen: `mdh observe`, `mdh tap "Sign in"`, `mdh type "text" --into Email`,
   `mdh scroll down --until "Item 30"`. Every action reports what changed, plus any crash.
4. `mdh verify 'screen .LoginActivity' 'enabled id=sign_in' 'visible "Welcome"'` — a verdict with
   evidence in `.mdh/runs/`; `no crash` is always checked.
5. `mdh flow run --changed` replays the saved flows that pass the affected screens. Save what you
   did as a flow with `mdh flow save NAME --check '…'`; flows live in `.mdh/flows/` (commit them).

Over MCP the same steps are `mdh_impact`, `mdh_run`, `mdh_observe`, `mdh_act`, `mdh_verify` and
`mdh_flow`.
"#;

/// Sets a project up: `.mdh/.gitignore` (keeps flows, ignores session, runs and cache),
/// `.mdh/flows/`, and with `agents_md` the section above in `AGENTS.md`. Returns what it did.
pub fn init(root: &Path, agents_md: bool) -> Result<Vec<String>> {
    let mut done = Vec::new();
    let mdh = root.join(".mdh");
    std::fs::create_dir_all(mdh.join("flows"))?;
    let ignore = mdh.join(".gitignore");
    if !ignore.exists() {
        std::fs::write(
            &ignore,
            "# mdh state and output; flows/ is committed\nsession.json\nruns/\ncache/\n",
        )?;
        done.push(format!("wrote {}", ignore.display()));
    }
    if agents_md {
        let path = root.join("AGENTS.md");
        let section = format!("{BEGIN}\n{AGENTS_SECTION}{END}\n");
        let (text, verb) = match std::fs::read_to_string(&path) {
            Ok(existing) => match (existing.find(BEGIN), existing.find(END)) {
                (Some(b), Some(e)) if b < e => (
                    format!(
                        "{}{section}{}",
                        &existing[..b],
                        existing[e + END.len()..].trim_start_matches('\n')
                    ),
                    "updated the mdh section of",
                ),
                _ => (
                    format!("{}\n\n{section}", existing.trim_end()),
                    "added an mdh section to",
                ),
            },
            Err(_) => (format!("# AGENTS.md\n\n{section}"), "wrote"),
        };
        std::fs::write(&path, text)?;
        done.push(format!("{verb} {}", path.display()));
    }
    Ok(done)
}

/// When the newest passing verdict under any of `runs` directories was written.
pub fn last_pass(runs: &[PathBuf]) -> Option<SystemTime> {
    runs.iter()
        .filter_map(|r| std::fs::read_dir(r).ok())
        .flatten()
        .flatten()
        .map(|e| e.path().join("verdict.json"))
        .filter(|p| {
            std::fs::read(p)
                .ok()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                .is_some_and(|v| v["status"] == "pass")
        })
        .filter_map(|p| p.metadata().ok()?.modified().ok())
        .max()
}

/// App files changed (uncommitted) after `since` and after the newest passing verdict: the work a
/// session would leave unverified. Empty outside a git repository or Gradle project.
pub fn unverified_changes(dir: &Path, since: Option<SystemTime>) -> Vec<String> {
    let Ok((root, files)) = mdh_impact::changed_app_files(dir) else {
        return Vec::new();
    };
    if !root.join("settings.gradle").is_file() && !root.join("settings.gradle.kts").is_file() {
        return Vec::new();
    }
    let pass = last_pass(&[root.join(".mdh/runs"), dir.join(".mdh/runs")]);
    let after = [since, pass].into_iter().flatten().max();
    files
        .into_iter()
        .filter(|f| {
            let modified = root.join(f).metadata().and_then(|m| m.modified()).ok();
            match (modified, after) {
                (Some(m), Some(a)) => m > a,
                (Some(_), None) => true,
                (None, _) => false,
            }
        })
        .collect()
}

/// Why the agent shouldn't stop yet, if it changed app files without verifying them afterwards.
pub fn stop_reason(dir: &Path, since: Option<SystemTime>) -> Option<String> {
    let files = unverified_changes(dir, since);
    if files.is_empty() {
        return None;
    }
    let names: Vec<&str> = files
        .iter()
        .take(5)
        .map(|f| f.rsplit('/').next().unwrap_or(f))
        .collect();
    let more = if files.len() > 5 {
        format!(" (+{} more)", files.len() - 5)
    } else {
        String::new()
    };
    Some(format!(
        "App changes made in this session have no passing verdict yet: {}{more}. Before finishing: \
         `mdh impact` (or mdh_impact) for the affected screens, `mdh run`, then verify each of them \
         with `mdh verify …` / `mdh flow run --changed` (mdh_verify) until the verdict passes. If it \
         can't be verified here (no device, needs real data), say so explicitly in your answer.",
        names.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_keeps_one_section_in_agents_md() {
        let dir = std::env::temp_dir().join(format!("mdh-init-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("AGENTS.md"), "# Rules\n\nBe nice.\n").unwrap();
        init(&dir, true).unwrap();
        let again = init(&dir, true).unwrap();
        let text = std::fs::read_to_string(dir.join("AGENTS.md")).unwrap();
        assert!(
            text.starts_with("# Rules\n\nBe nice.\n\n<!-- mdh:begin -->\n## Verifying changes"),
            "{text}"
        );
        assert_eq!(text.matches(BEGIN).count(), 1, "{text}");
        assert!(text.ends_with("<!-- mdh:end -->\n"), "{text}");
        assert_eq!(again.len(), 1, "the .gitignore isn't rewritten: {again:?}");
        assert!(dir.join(".mdh/flows").is_dir());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
