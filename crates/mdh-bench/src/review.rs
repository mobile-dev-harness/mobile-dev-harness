//! Task review by a model from another family than the author's (bench/DESIGN.md, 3.4).

use std::path::Path;
use std::process::Command;

use crate::agent::Provider;
use crate::task::Task;
use crate::workspace;

const CHECKLIST: &str = "\
You are reviewing one task of a benchmark for coding agents on Android, before it is frozen. The \
benchmark asks whether an agent's own claim (\"it works\", \"it's fixed\") is true, and what device \
access changes about that. Read bench/DESIGN.md's rules as summarized here; you can't change \
anything, only read.

In the current directory:
- `app/` is the workspace an agent starts from: the app with the task's defect committed (fix \
tasks) or the change to judge left uncommitted (verify tasks; `git -C app diff` shows it).
- `task/task.yaml` is the task: the prompt the agent gets, its level and source, the defect \
(`bug`), the change (`change`), the reference fix (`fix`) and other correct fixes \
(`alternatives`), as literal find/replace edits.
- `task/checks/` are the hidden checks the grader replays (mdh flows, YAML); the agent never \
sees them.
- `task/pair.yaml`, when present, is the other half of a verify pair.

Levels, by the least evidence that decides the task: L0 the code or the diff; L1 running the app \
on one path; L2 running it under a condition (dark mode, font scale, time zone, rotation, window \
size, keyboard...) or over several steps; L3 measuring (jank, startup), a few pixels of layout, \
another API level, or a screen away from the change. For L1 and above, an agent reading the code \
without a device should not be able to tell reliably; a fix task above L0 needs an obvious-looking \
fix that doesn't work. The device runs in America/Los_Angeles, en-US, light mode, portrait.

Check each item and answer it with OK or PROBLEM and a reason of one or two sentences:
1. The prompt says what its reporter (a user, or a PR's author for verify tasks) would know, and \
not the cause or the fix.
2. The defect or change matches the prompt, and nothing else in the app was broken by it.
3. The reference fix fixes it; it is not the only reasonable fix, and the alternatives listed are \
correct. Name a correct fix the checks would wrongly reject, if you see one.
4. The checks test behavior a user would see, not the implementation; they fail on the defect \
(or the broken change) and pass on the fix (or the correct change).
5. The level holds: say what evidence decides the task, and whether reading the code alone \
would.
6. Nothing in the workspace gives the answer away (comments, names, test files, leftovers).
7. Anything else that makes the task unfair, ambiguous or flaky.

End with one line: `REVIEW: ACCEPT` or `REVIEW: REVISE`.";

pub fn run(repo: &Path, provider: &str, names: &[String], again: bool) -> Result<(), String> {
    let provider = Provider::load(provider)?;
    let apps = crate::app::load_all(repo)?;
    let tasks = crate::selected(repo, names)?;
    // The other half of a pair is shown to the reviewer whether or not it was selected.
    let all = crate::selected(repo, &[])?;
    let sdk = std::env::var_os("ANDROID_HOME").map_or_else(
        || Path::new(&std::env::var("HOME").unwrap_or_default()).join("Library/Android/sdk"),
        Into::into,
    );
    for t in &tasks {
        let out = t.dir.join("review.md");
        if out.exists() && !again {
            println!("{}: reviewed already ({})", t.id, out.display());
            continue;
        }
        let dir = crate::scratch(&format!("review-{}", t.id));
        let _ = std::fs::remove_dir_all(&dir);
        let source = apps[&t.app].source(repo)?;
        workspace::prepare(&source, &dir.join("app"), t, &sdk, None)?;
        copy_task(t, &all, &dir.join("task"))?;
        eprintln!(
            "[{}] reviewing {} with {} …",
            crate::now(),
            t.id,
            provider.model
        );
        let output = Command::new("claude")
            .envs(provider.env_vars())
            .current_dir(&dir)
            .args(["-p", CHECKLIST, "--model", &provider.model])
            .args(["--output-format", "json", "--no-session-persistence"])
            .args(["--setting-sources", "", "--strict-mcp-config"])
            .args(["--permission-mode", "bypassPermissions"])
            .args(["--disallowedTools", "Edit", "Write", "NotebookEdit"])
            .args(["WebFetch", "WebSearch"])
            .output()
            .map_err(|e| format!("claude: {e}"))?;
        let v: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_default();
        let Some(text) = v["result"].as_str().filter(|t| !t.trim().is_empty()) else {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(format!(
                "{}: no review: {}",
                t.id,
                String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .next()
                    .unwrap_or("no output")
            ));
        };
        let verdict = text
            .lines()
            .rev()
            .find_map(|l| l.trim().trim_matches(['*', '`']).strip_prefix("REVIEW:"))
            .map_or("none", str::trim);
        let date = Command::new("date")
            .arg("+%F")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .unwrap_or_default();
        std::fs::write(
            &out,
            format!(
                "<!-- Reviewed by {} through {} on {date}; regenerate with `mdh-bench review --again --tasks {}`. -->\n\n{}\n",
                provider.model,
                provider.name,
                t.id,
                text.trim()
            ),
        )
        .map_err(|e| e.to_string())?;
        println!("{}: {verdict} ({})", t.id, out.display());
        let _ = std::fs::remove_dir_all(&dir);
    }
    Ok(())
}

/// The task's files for the reviewer, with the other half of its pair.
fn copy_task(t: &Task, all: &[Task], to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to.join("checks")).map_err(|e| e.to_string())?;
    let copy =
        |from: &Path, to: &Path| std::fs::copy(from, to).map(drop).map_err(|e| e.to_string());
    copy(&t.dir.join("task.yaml"), &to.join("task.yaml"))?;
    for c in t.checks() {
        copy(
            &c,
            &to.join("checks").join(c.file_name().unwrap_or_default()),
        )?;
    }
    if let Some(other) = t
        .pair
        .as_deref()
        .and_then(|p| all.iter().find(|o| o.id == p))
    {
        copy(&other.dir.join("task.yaml"), &to.join("pair.yaml"))?;
    }
    Ok(())
}
