//! `mdh-bench`: runs the benchmark's tasks with a coding agent in each setup, grades them, and
//! reports false passes, false fails, tokens, tool calls and time (milestone M10).

mod agent;
mod env;
mod grade;
mod report;
mod setup;
mod task;
mod workspace;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};

use crate::env::Env;
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
        #[arg(long, default_value = "claude-sonnet-5-5")]
        model: String,
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
            budget,
            max_run_usd,
            timeout_min,
        } => run(
            &cli.repo,
            &out,
            &tasks,
            &setups,
            reps,
            agent::AgentOptions {
                model,
                max_usd: max_run_usd,
                timeout: Duration::from_secs(timeout_min * 60),
            },
            budget,
        ),
        Cmd::Report { out } => report(&out),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn selected(repo: &Path, names: &[String]) -> Result<Vec<Task>, String> {
    let all = task::load_all(&repo.join("bench/tasks"))?;
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
    for t in task::load_all(&repo.join("bench/tasks"))? {
        let truth = match (t.kind, t.truth) {
            (Kind::Verify, Some(Truth::Pass)) => "verify, correct",
            (Kind::Verify, _) => "verify, broken",
            (Kind::Fix, _) => "fix",
        };
        println!("{:32} {truth:16} {}", t.id, t.summary);
    }
    Ok(())
}

fn scratch(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("mdh-bench-{name}-{}", std::process::id()))
}

fn validate(repo: &Path, names: &[String]) -> Result<(), String> {
    let env = Env::detect(repo)?;
    let mut bad = 0;
    for t in selected(repo, names)? {
        let mut cases = vec![(
            false,
            t.kind == Kind::Verify && t.truth == Some(Truth::Pass),
        )];
        if t.kind == Kind::Fix {
            cases.push((true, true));
        }
        for (fixed, expect) in cases {
            let dir = scratch(&t.id);
            let _ = std::fs::remove_dir_all(&dir);
            let app = dir.join("app");
            workspace::prepare(&env.sample(), &app, &t, fixed)?;
            let checks = grade::checks(&env, &t, &app, &dir.join("grade"));
            let ok = checks.passed == expect;
            if !ok {
                bad += 1;
            }
            println!(
                "{} {}{}: checks {} (expected {}) — {}",
                if ok { "ok  " } else { "BAD " },
                t.id,
                if fixed { " + reference fix" } else { "" },
                if checks.passed { "pass" } else { "fail" },
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
    workspace::prepare(&env.sample(), &app, t, false)?;
    let before = workspace::snapshot(&app);
    env.reset_device();
    let usage = agent::run(env, s, t, &app, &dir.join("agent.log"), options);
    let answer = agent::answer(t.kind, &usage.result);
    let edited = t.kind == Kind::Verify && workspace::snapshot(&app) != before;
    let (works, detail) = match t.kind {
        Kind::Fix => {
            let c = grade::checks(env, t, &app, &dir.join("grade"));
            (Some(c.passed), c.detail)
        }
        Kind::Verify => (None, String::new()),
    };
    env.reset_device();
    let outcome = grade::outcome(t, answer, works.unwrap_or(false));
    Ok(Record {
        task: t.id.clone(),
        kind: t.kind,
        setup: s.key().to_owned(),
        rep,
        outcome,
        answer,
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

fn now() -> String {
    std::process::Command::new("date")
        .arg("+%H:%M:%S")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}
