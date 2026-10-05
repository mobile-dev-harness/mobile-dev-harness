//! `mdh-bench`: runs the benchmark's tasks with a coding agent in each setup, grades them, and
//! reports false passes, false fails, tokens, tool calls and time (milestone M10).

mod agent;
mod app;
mod env;
mod grade;
mod report;
mod review;
mod setup;
mod task;
mod workspace;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};

use crate::env::Env;
use crate::grade::Outcome;
use crate::report::Record;
use crate::setup::Setup;
use crate::task::{Kind, Task, Truth};

#[derive(Parser)]
#[command(name = "mdh-bench", about)]
struct Cli {
    /// The repository root
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List the tasks
    List,
    /// Check every task's hidden checks against its truth: the seeded bug fails them, the
    /// reference fix (or a correct change) passes them
    Validate {
        #[arg(long, value_delimiter = ',')]
        tasks: Vec<String>,
    },
    /// Run tasks × setups × repetitions with the agent and grade them
    Run {
        /// Where results go (appended to; a run already recorded is skipped)
        #[arg(long)]
        out: PathBuf,
        #[arg(long, value_delimiter = ',')]
        tasks: Vec<String>,
        /// a (agent alone), b (adb), c (mobile-mcp), d (mdh)
        #[arg(long, value_delimiter = ',', default_value = "a,b,c,d")]
        setups: Vec<String>,
        #[arg(long, default_value_t = 1)]
        reps: usize,
        /// Ignored with --provider, whose file names the model
        #[arg(long, default_value = "claude-sonnet-5-5")]
        model: String,
        /// Another Anthropic-compatible provider, configured in ~/.config/mdh-bench/<name>.env
        #[arg(long)]
        provider: Option<String>,
        /// Stop starting runs once this much is spent, in USD
        #[arg(long, default_value_t = 300.0)]
        budget: f64,
        /// Per run, in USD
        #[arg(long, default_value_t = 5.0)]
        max_run_usd: f64,
        /// Per run, in minutes
        #[arg(long, default_value_t = 25)]
        timeout_min: u64,
    },
    /// Print the results table of a run directory
    Report { out: PathBuf },
    /// Grade recorded fix runs again from their kept workspaces, and read every run's transcript
    /// again (unchecked claims); results.jsonl is rewritten, the old one kept as results.jsonl.bak
    Regrade {
        out: PathBuf,
        #[arg(long, value_delimiter = ',')]
        tasks: Vec<String>,
        /// Only runs graded as not working or as grader errors
        #[arg(long)]
        failed_only: bool,
        /// Read the transcripts only; no device needed
        #[arg(long)]
        transcripts_only: bool,
    },
    /// Have a model from another family review tasks against the checklist (bench/DESIGN.md, 3.4);
    /// writes bench/tasks/<id>/review.md
    Review {
        #[arg(long, default_value = "glm")]
        provider: String,
        #[arg(long, value_delimiter = ',')]
        tasks: Vec<String>,
        /// Review tasks that already have a review
        #[arg(long)]
        again: bool,
    },
    /// Check a provider before a run: it answers, calls a tool, and reads an image (screenshots)
    Probe {
        #[arg(long)]
        provider: Option<String>,
        #[arg(long, default_value = "claude-sonnet-5-5")]
        model: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Cmd::List => list(&cli.repo),
        Cmd::Validate { tasks } => validate(&cli.repo, &tasks),
        Cmd::Run {
            out,
            tasks,
            setups,
            reps,
            model,
            provider,
            budget,
            max_run_usd,
            timeout_min,
        } => provider
            .as_deref()
            .map(agent::Provider::load)
            .transpose()
            .and_then(|provider| {
                run(
                    &cli.repo,
                    &out,
                    &tasks,
                    &setups,
                    reps,
                    agent::AgentOptions {
                        model: provider.as_ref().map_or(model, |p| p.model.clone()),
                        provider,
                        max_usd: max_run_usd,
                        timeout: Duration::from_secs(timeout_min * 60),
                    },
                    budget,
                )
            }),
        Cmd::Report { out } => report(&out),
        Cmd::Regrade {
            out,
            tasks,
            failed_only,
            transcripts_only,
        } => regrade(&cli.repo, &out, &tasks, failed_only, transcripts_only),
        Cmd::Review {
            provider,
            tasks,
            again,
        } => review::run(&cli.repo, &provider, &tasks, again),
        Cmd::Probe { provider, model } => probe(provider.as_deref(), &model),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn all_tasks(repo: &Path) -> Result<Vec<Task>, String> {
    task::load_all(&repo.join("bench/tasks"), &app::load_all(repo)?)
}

pub(crate) fn selected(repo: &Path, names: &[String]) -> Result<Vec<Task>, String> {
    let all = all_tasks(repo)?;
    if names.is_empty() {
        return Ok(all);
    }
    for n in names {
        if !all.iter().any(|t| &t.id == n) {
            return Err(format!("no task {n}"));
        }
    }
    Ok(all.into_iter().filter(|t| names.contains(&t.id)).collect())
}

fn list(repo: &Path) -> Result<(), String> {
    for t in all_tasks(repo)? {
        let truth = match (t.kind, t.truth) {
            (Kind::Verify, Some(Truth::Pass)) => "verify, correct",
            (Kind::Verify, _) => "verify, broken",
            (Kind::Fix, _) => "fix",
        };
        let level = t.level.map_or("–".into(), |l| format!("{l:?}"));
        let origin = match (&t.upstream, t.source) {
            (Some(u), _) => u.clone(),
            (None, Some(s)) => format!("{s:?}").to_lowercase(),
            (None, None) => String::new(),
        };
        println!(
            "{:32} {:13} {level:3} {truth:16} {}{}{}",
            t.id,
            t.app,
            t.summary,
            if origin.is_empty() {
                String::new()
            } else {
                format!(" [{origin}]")
            },
            if t.private { " (private)" } else { "" }
        );
    }
    Ok(())
}

pub(crate) fn scratch(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("mdh-bench-{name}-{}", std::process::id()))
}

fn validate(repo: &Path, names: &[String]) -> Result<(), String> {
    let env = Env::detect(repo)?;
    let mut bad = 0;
    for t in selected(repo, names)? {
        let source = env.app(&t.app).source(&env.repo)?;
        // (what is applied on top, its name, whether the checks must pass)
        let mut cases: Vec<(Option<&[task::Edit]>, String, bool)> = vec![(
            None,
            String::new(),
            t.kind == Kind::Verify && t.truth == Some(Truth::Pass),
        )];
        if t.kind == Kind::Fix {
            cases.push((Some(&t.fix), " + reference fix".into(), true));
            for (i, alt) in t.alternatives.iter().enumerate() {
                cases.push((Some(alt), format!(" + alternative fix {}", i + 1), true));
            }
        }
        for (fix, name, expect) in cases {
            let dir = scratch(&t.id);
            let _ = std::fs::remove_dir_all(&dir);
            let app = dir.join("app");
            workspace::prepare(&source, &app, &t, &env.sdk, fix)?;
            let checks = grade::checks(&env, &t, &app, &dir.join("grade"));
            let ok = !checks.error && checks.passed == expect;
            if !ok {
                bad += 1;
            }
            println!(
                "{} {}{name}: checks {} (expected {}) — {}",
                if ok { "ok  " } else { "BAD " },
                t.id,
                match (checks.error, checks.passed) {
                    (true, _) => "error",
                    (false, true) => "pass",
                    (false, false) => "fail",
                },
                if expect { "pass" } else { "fail" },
                checks.detail
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
    env.reset_device();
    if bad > 0 {
        Err(format!("{bad} task case(s) don't grade as expected"))
    } else {
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    repo: &Path,
    out: &Path,
    names: &[String],
    setups: &[String],
    reps: usize,
    options: agent::AgentOptions,
    budget: f64,
) -> Result<(), String> {
    let env = Env::detect(repo)?;
    let tasks = selected(repo, names)?;
    let setups = setups
        .iter()
        .map(|s| Setup::parse(s).ok_or_else(|| format!("unknown setup {s}")))
        .collect::<Result<Vec<_>, _>>()?;
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    // The agent runs elsewhere: every path it's given must be absolute.
    let out = &out.canonicalize().map_err(|e| e.to_string())?;
    let env = Env {
        plugin: snapshot_plugin(&env.plugin, out)?,
        ..env
    };
    eprintln!("device: {}", env.device);
    let results = out.join("results.jsonl");
    let done = load(&results);
    let mut spent: f64 = done.iter().map(|r| r.cost_usd).sum();
    // Repetitions outermost and setups innermost: drift over hours (the emulator, the network)
    // spreads evenly over the setups.
    for rep in 1..=reps {
        for t in &tasks {
            for &s in &setups {
                if done
                    .iter()
                    .any(|r| r.task == t.id && r.setup == s.key() && r.rep == rep)
                {
                    continue;
                }
                if spent >= budget {
                    eprintln!("budget of ${budget:.0} reached (${spent:.2}); stopping");
                    return Ok(());
                }
                eprintln!("[{}] {} · {} · run {rep} …", now(), t.id, s.name());
                let record = one(&env, out, t, s, rep, &options)?;
                spent += record.cost_usd;
                eprintln!(
                    "[{}]   {:?} · ${:.2} · {} tool calls · {:.1} min{} · total ${spent:.2}",
                    now(),
                    record.outcome,
                    record.cost_usd,
                    record.tool_calls,
                    record.duration_ms as f64 / 60_000.0,
                    record
                        .error
                        .as_deref()
                        .map(|e| format!(" · {e}"))
                        .unwrap_or_default()
                );
                let mut f = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&results)
                    .map_err(|e| e.to_string())?;
                writeln!(
                    f,
                    "{}",
                    serde_json::to_string(&record).expect("serializable")
                )
                .map_err(|e| e.to_string())?;
            }
        }
    }
    report(out)
}

fn one(
    env: &Env,
    out: &Path,
    t: &Task,
    s: Setup,
    rep: usize,
    options: &agent::AgentOptions,
) -> Result<Record, String> {
    let dir = out.join(&t.id).join(format!("{}-{rep}", s.key()));
    let _ = std::fs::remove_dir_all(&dir);
    let app = dir.join("app");
    let source = env.app(&t.app).source(&env.repo)?;
    workspace::prepare(&source, &app, t, &env.sdk, None)?;
    let before = workspace::snapshot(&app);
    env.reset_device();
    env.check_device()?;
    let usage = agent::run(env, s, t, &app, &dir.join("agent.log"), options);
    // A run whose agent replaced or lost the device isn't recorded, and the ones after it would
    // run elsewhere: stop.
    env.check_device().map_err(|e| {
        format!(
            "{e} during {}; restore it and run again (recorded runs are kept)",
            dir.display()
        )
    })?;
    let answer = agent::answer(t.kind, &usage.result);
    let edited = t.kind == Kind::Verify && workspace::snapshot(&app) != before;
    let (works, detail) = match t.kind {
        Kind::Fix => {
            let c = grade::checks(env, t, &app, &dir.join("grade"));
            ((!c.error).then_some(c.passed), c.detail)
        }
        Kind::Verify => (None, String::new()),
    };
    env.reset_device();
    let outcome = grade::outcome(
        t,
        answer,
        match t.kind {
            Kind::Fix => works,
            Kind::Verify => Some(t.truth == Some(Truth::Pass)),
        },
    );
    Ok(Record {
        task: t.id.clone(),
        kind: t.kind,
        setup: s.key().to_owned(),
        model: options.model.clone(),
        provider: options.provider.as_ref().map(|p| p.name.clone()),
        device: Some(env.device.to_string()),
        prompt: agent::PROMPT_VERSION,
        app: Some(t.app.clone()),
        level: t.level,
        source: t.source,
        checked: Some(usage.checked()),
        truth: match t.kind {
            Kind::Fix => works,
            Kind::Verify => Some(t.truth == Some(Truth::Pass)),
        },
        rep,
        outcome,
        answer: answer.and_then(|a| match a {
            grade::Answer::Works => Some(true),
            grade::Answer::Broken => Some(false),
            grade::Answer::Unverified => None,
        }),
        works,
        detail,
        edited,
        cost_usd: usage.cost_usd,
        tokens: usage.tokens(),
        output_tokens: usage.output_tokens,
        tool_calls: usage.tool_calls,
        images: usage.images,
        turns: usage.turns,
        duration_ms: usage.duration_ms,
        timed_out: usage.timed_out,
        error: usage.error.clone().or_else(|| {
            usage
                .mcp
                .iter()
                .find(|(_, status)| status != "connected")
                .map(|(n, st)| format!("MCP server {n}: {st}"))
        }),
    })
}

/// The plugin as of the run's start, kept with the results: edits to the repository's copy
/// mid-run (or between resumed sessions) would otherwise split setup D's runs across versions.
fn snapshot_plugin(source: &Path, out: &Path) -> Result<PathBuf, String> {
    let copy = out.join("plugin");
    if copy.is_dir() {
        let same = std::process::Command::new("diff")
            .args(["-rq"])
            .args([source, &copy])
            .output()
            .is_ok_and(|o| o.status.success());
        if !same {
            eprintln!(
                "note: {} differs from {}; runs keep using the copy taken when this run started",
                source.display(),
                copy.display()
            );
        }
        return Ok(copy);
    }
    let ok = std::process::Command::new("cp")
        .arg("-R")
        .args([source, &copy])
        .status()
        .is_ok_and(|s| s.success());
    if ok {
        Ok(copy)
    } else {
        Err(format!(
            "couldn't copy {} to {}",
            source.display(),
            copy.display()
        ))
    }
}

fn probe(provider: Option<&str>, model: &str) -> Result<(), String> {
    let provider = provider.map(agent::Provider::load).transpose()?;
    let model = provider
        .as_ref()
        .map_or(model.to_owned(), |p| p.model.clone());
    let dir = scratch("probe");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // A 64×64 red PNG: can the model see screenshots?
    let png: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x40, 0x08, 0x02, 0x00, 0x00, 0x00, 0x25,
        0x0B, 0xE6, 0x89, 0x00, 0x00, 0x00, 0x7F, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0xD5, 0xCE,
        0x41, 0x11, 0x00, 0x20, 0x0C, 0xC0, 0xB0, 0x52, 0x21, 0xF3, 0x2F, 0x0A, 0x31, 0x88, 0xE0,
        0xB1, 0x6B, 0x14, 0xE4, 0xDC, 0x19, 0xCA, 0x24, 0x4E, 0xE2, 0x24, 0x4E, 0xE2, 0x24, 0x4E,
        0xE2, 0x24, 0x4E, 0xE2, 0x24, 0x4E, 0xE2, 0x24, 0x4E, 0xE2, 0x24, 0x4E, 0xE2, 0x24, 0x4E,
        0xE2, 0x24, 0x4E, 0xE2, 0x24, 0x4E, 0xE2, 0x24, 0x4E, 0xE2, 0x24, 0x4E, 0xE2, 0x24, 0x4E,
        0xE2, 0x24, 0x4E, 0xE2, 0x24, 0x4E, 0xE2, 0x24, 0x4E, 0xE2, 0x24, 0x4E, 0xE2, 0x24, 0x4E,
        0xE2, 0x24, 0x4E, 0xE2, 0x24, 0x4E, 0xE2, 0x24, 0x4E, 0xE2, 0x24, 0x4E, 0xE2, 0x24, 0x4E,
        0xE2, 0x24, 0x4E, 0xE2, 0x24, 0x4E, 0xE2, 0x24, 0x4E, 0xE2, 0x24, 0x4E, 0xE2, 0x24, 0x4E,
        0xE2, 0x24, 0x4E, 0xE2, 0x24, 0x4E, 0xE2, 0x24, 0x4E, 0xE2, 0xDC, 0x0E, 0xFC, 0x7A, 0x08,
        0x9D, 0x01, 0x98, 0xF0, 0x16, 0xB9, 0x60, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44,
        0xAE, 0x42, 0x60, 0x82,
    ];
    std::fs::write(dir.join("square.png"), png).map_err(|e| e.to_string())?;
    let checks = [
        (
            "answers",
            "Reply with the single word: ready".to_owned(),
            "ready",
        ),
        (
            "calls a tool",
            "Use the Bash tool to run `echo mdh-$((6*7))` and reply with exactly its output."
                .to_owned(),
            "mdh-42",
        ),
        (
            "reads an image",
            format!(
                "Use the Read tool to look at {} and reply with its color in one lowercase word.",
                dir.join("square.png").display()
            ),
            "red",
        ),
    ];
    for (what, prompt, expect) in checks {
        let mut cmd = std::process::Command::new("claude");
        if let Some(p) = &provider {
            cmd.envs(p.env_vars());
        }
        let out = cmd
            .current_dir(&dir)
            .args(["-p", &prompt, "--model", &model, "--output-format", "json"])
            .args([
                "--no-session-persistence",
                "--setting-sources",
                "",
                "--strict-mcp-config",
            ])
            .args(["--permission-mode", "bypassPermissions"])
            .output()
            .map_err(|e| format!("claude: {e}"))?;
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
        let result = v["result"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        let models: Vec<&String> = v["modelUsage"]
            .as_object()
            .map(|m| m.keys().collect())
            .unwrap_or_default();
        let ok = result.contains(expect);
        println!(
            "{} {what}: {:?}{}",
            if ok { "ok  " } else { "FAIL" },
            result.chars().take(80).collect::<String>(),
            if models.is_empty() {
                format!(
                    " ({})",
                    String::from_utf8_lossy(&out.stderr)
                        .lines()
                        .next()
                        .unwrap_or("no output")
                )
            } else {
                format!(
                    " — model {}",
                    models
                        .iter()
                        .map(|m| m.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

fn regrade(
    repo: &Path,
    out: &Path,
    names: &[String],
    failed_only: bool,
    transcripts_only: bool,
) -> Result<(), String> {
    let results = out.join("results.jsonl");
    let mut records = load(&results);
    if records.is_empty() {
        return Err(format!("no results in {}", out.display()));
    }
    let tasks = all_tasks(repo)?;
    let env = if transcripts_only {
        None
    } else {
        Some(Env::detect(repo)?)
    };
    let mut changed = 0;
    for r in &mut records {
        if !names.is_empty() && !names.contains(&r.task) {
            continue;
        }
        let Some(t) = tasks.iter().find(|t| t.id == r.task) else {
            eprintln!("{}: no such task any more, left as it was", r.task);
            continue;
        };
        let dir = out.join(&r.task).join(format!("{}-{}", r.setup, r.rep));
        let usage = agent::read_transcript(&dir.join("agent.jsonl"), &dir.join("app"));
        r.checked = Some(usage.checked());
        let answer = match (r.outcome, r.answer) {
            (Outcome::Abstained, _) => Some(grade::Answer::Unverified),
            (_, Some(true)) => Some(grade::Answer::Works),
            (_, Some(false)) => Some(grade::Answer::Broken),
            (_, None) => None,
        };
        let failed = matches!(r.works, Some(false) | None);
        if let Some(env) = &env
            && t.kind == Kind::Fix
            && (!failed_only || failed)
        {
            env.check_device()?;
            eprintln!(
                "[{}] regrading {} · {} · run {} …",
                now(),
                r.task,
                r.setup,
                r.rep
            );
            let c = grade::checks(env, t, &dir.join("app"), &dir.join("grade"));
            let works = (!c.error).then_some(c.passed);
            if works != r.works {
                eprintln!("  {:?} → {:?}: {}", r.works, works, c.detail);
            }
            r.works = works;
            r.detail = c.detail;
        }
        r.truth = match t.kind {
            Kind::Fix => r.works,
            Kind::Verify => Some(t.truth == Some(Truth::Pass)),
        };
        let outcome = grade::outcome(t, answer, r.truth);
        if outcome != r.outcome {
            changed += 1;
            eprintln!(
                "{} · {} · run {}: {:?} → {outcome:?}",
                r.task, r.setup, r.rep, r.outcome
            );
        }
        r.outcome = outcome;
    }
    if let Some(env) = &env {
        env.reset_device();
    }
    let backup = out.join("results.jsonl.bak");
    if !backup.exists() {
        std::fs::copy(&results, &backup).map_err(|e| e.to_string())?;
    }
    let text: String = records
        .iter()
        .map(|r| serde_json::to_string(r).expect("serializable") + "\n")
        .collect();
    let tmp = out.join("results.jsonl.tmp");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &results).map_err(|e| e.to_string())?;
    eprintln!("{changed} outcome(s) changed");
    report(out)
}

fn load(path: &Path) -> Vec<Record> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn report(out: &Path) -> Result<(), String> {
    let records = load(&out.join("results.jsonl"));
    if records.is_empty() {
        return Err(format!("no results in {}", out.display()));
    }
    let table = report::markdown(&records, &setup::ALL);
    std::fs::write(out.join("report.md"), &table).map_err(|e| e.to_string())?;
    println!("{table}");
    Ok(())
}

pub(crate) fn now() -> String {
    std::process::Command::new("date")
        .arg("+%H:%M:%S")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}
