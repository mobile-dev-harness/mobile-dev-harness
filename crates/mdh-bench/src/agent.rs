//! Running the agent (Claude Code, headless) and reading what it did from its event stream.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::env::Env;
use crate::setup::Setup;
use crate::task::{Kind, Task};

#[derive(Debug, Clone)]
pub struct AgentOptions {
    pub model: String,
    /// Another Anthropic-compatible provider: the environment Claude Code needs to reach it
    /// (`ANTHROPIC_BASE_URL`, `ANTHROPIC_AUTH_TOKEN`, model overrides). Never logged.
    pub provider: Option<Provider>,
    /// Per run, passed to `--max-budget-usd`.
    pub max_usd: f64,
    pub timeout: Duration,
}

/// A model provider other than Anthropic, read from a `KEY=VALUE` file outside the repository
/// (`~/.config/mdh-bench/<name>.env`): the variables its documentation gives for Claude Code, plus
/// `MODEL`, the model to pass to `--model`.
#[derive(Debug, Clone)]
pub struct Provider {
    pub name: String,
    pub model: String,
    env: Vec<(String, String)>,
}

impl Provider {
    pub fn env_vars(&self) -> impl Iterator<Item = (&str, &str)> {
        self.env.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn load(name: &str) -> Result<Provider, String> {
        let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
        let path = Path::new(&home).join(format!(".config/mdh-bench/{name}.env"));
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)
                .map(|m| m.permissions().mode())
                .unwrap_or(0);
            if mode & 0o077 != 0 {
                return Err(format!(
                    "{} is readable by others; run chmod 600 on it",
                    path.display()
                ));
            }
        }
        let mut env = Vec::new();
        let mut model = None;
        for line in text.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let line = line.strip_prefix("export ").unwrap_or(line);
            let Some((k, v)) = line.split_once('=') else {
                return Err(format!("{}: `{line}` is not KEY=VALUE", path.display()));
            };
            let v = v.trim().trim_matches(['"', '\'']).to_owned();
            if k.trim() == "MODEL" {
                model = Some(v);
            } else {
                env.push((k.trim().to_owned(), v));
            }
        }
        if !env.iter().any(|(k, _)| k == "ANTHROPIC_BASE_URL") {
            return Err(format!("{}: needs ANTHROPIC_BASE_URL", path.display()));
        }
        Ok(Provider {
            name: name.to_owned(),
            model: model.ok_or_else(|| format!("{}: needs MODEL", path.display()))?,
            env,
        })
    }
}

/// What a run cost and did.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Usage {
    pub cost_usd: f64,
    pub input_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub output_tokens: u64,
    pub turns: u64,
    pub tool_calls: u64,
    /// Calls by tool name.
    pub tools: std::collections::BTreeMap<String, u64>,
    /// Images the agent looked at (screenshots).
    pub images: u64,
    pub duration_ms: u64,
    /// MCP servers and whether they connected, from the session's start.
    pub mcp: Vec<(String, String)>,
    /// The agent's final message.
    pub result: String,
    pub timed_out: bool,
    pub error: Option<String>,
}

impl Usage {
    pub fn tokens(&self) -> u64 {
        self.input_tokens + self.cache_creation_tokens + self.cache_read_tokens + self.output_tokens
    }
}

const COMMON: &str = "You are working on an Android app (Kotlin, Gradle) in the current directory; build \
it with ./gradlew assembleDebug. The app is a demo: some screens (Troubles, Overlap, Permissions, \
buttons that crash or freeze on purpose) contain deliberate problems unrelated to your task; ignore \
them unless your task is about them. Work on your own: nobody will answer questions.";

pub fn prompt(task: &Task) -> String {
    let ending = match task.kind {
        Kind::Verify => {
            "Don't change the code. When you are done, end your final message with one line, \
             `VERDICT: PASS` if the change works and breaks nothing, or `VERDICT: FAIL` if it doesn't, \
             followed by one sentence saying why."
        }
        Kind::Fix => {
            "When you are done, end your final message with one line, `RESULT: FIXED` if you fixed it \
             and checked the fix, or `RESULT: NOT FIXED`, followed by one sentence saying why."
        }
    };
    format!("{}\n{ending}", task.prompt.trim_end())
}

pub fn run(
    env: &Env,
    setup: Setup,
    task: &Task,
    workspace: &Path,
    log: &Path,
    options: &AgentOptions,
) -> Usage {
    let mcp = log.with_file_name("mcp.json");
    let _ = std::fs::write(&mcp, setup.mcp(env).to_string());
    let system = format!("{COMMON}\n{}", setup.brief(env));
    let mut cmd = Command::new("claude");
    if let Some(p) = &options.provider {
        cmd.envs(p.env_vars());
    }
    cmd.current_dir(workspace)
        .env("PATH", setup.path(env))
        .env("ANDROID_HOME", &env.sdk)
        .args(["-p", &prompt(task)])
        .args(["--model", &options.model])
        .args(["--output-format", "stream-json", "--verbose"])
        .args(["--no-session-persistence", "--setting-sources", ""])
        .args(["--strict-mcp-config", "--mcp-config"])
        .arg(&mcp)
        .args(["--append-system-prompt", &system])
        .args(["--permission-mode", "bypassPermissions"])
        .args(["--max-budget-usd", &format!("{:.2}", options.max_usd)])
        .args(["--disallowedTools", "WebFetch", "WebSearch"])
        // The device is shared by every run: no setup may replace it (see `Env::check_device`).
        .args([
            "Bash(*emulator -avd*)",
            "Bash(*emulator @*)",
            "Bash(*adb kill-server*)",
            "Bash(*adb emu *)",
        ]);
    if setup == Setup::Alone {
        cmd.args(["Bash(adb:*)", "Bash(*/adb *)", "Bash(emulator:*)"]);
    }
    if setup == Setup::Mdh {
        cmd.arg("--plugin-dir").arg(&env.plugin);
    }
    let started = Instant::now();
    let mut usage = Usage::default();
    let file = match std::fs::File::create(log) {
        Ok(f) => f,
        Err(e) => {
            usage.error = Some(e.to_string());
            return usage;
        }
    };
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = match cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(file)
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            usage.error = Some(format!("claude: {e}"));
            return usage;
        }
    };
    let stdout = child.stdout.take().expect("piped");
    // The agent and its MCP servers form one process group: a timeout ends them all.
    let group = format!("-{}", child.id());
    let leftover = group.clone();
    let (done, finished) = std::sync::mpsc::channel::<()>();
    let timeout = options.timeout;
    let watchdog = std::thread::spawn(move || {
        if finished.recv_timeout(timeout).is_err() {
            let _ = Command::new("kill").args(["-TERM", "--", &group]).status();
            return true;
        }
        false
    });
    let events = log.with_extension("jsonl");
    let mut out = std::fs::File::create(&events).ok();
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if let Some(f) = out.as_mut() {
            use std::io::Write;
            let _ = writeln!(f, "{line}");
        }
        if let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) {
            read_event(&event, &mut usage);
        }
    }
    let _ = child.wait();
    let _ = done.send(());
    // MCP servers the agent left running.
    let _ = Command::new("kill")
        .args(["-TERM", "--", &leftover])
        .stderr(Stdio::null())
        .status();
    usage.timed_out = watchdog.join().unwrap_or(false);
    if usage.duration_ms == 0 {
        usage.duration_ms = started.elapsed().as_millis() as u64;
        if usage.timed_out {
            usage.error = Some(format!("timed out after {} min", timeout.as_secs() / 60));
        } else if usage.error.is_none() {
            let why = std::fs::read_to_string(log).unwrap_or_default();
            usage.error = Some(format!(
                "no result from the agent: {}",
                why.lines()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("see agent.log")
            ));
        }
    }
    usage
}

fn read_event(e: &serde_json::Value, u: &mut Usage) {
    let n = |v: &serde_json::Value| v.as_u64().unwrap_or(0);
    match e["type"].as_str() {
        Some("system") if e["subtype"] == "init" => {
            for s in e["mcp_servers"].as_array().into_iter().flatten() {
                u.mcp.push((
                    s["name"].as_str().unwrap_or("?").to_owned(),
                    s["status"].as_str().unwrap_or("?").to_owned(),
                ));
            }
        }
        Some("assistant") => {
            for c in e["message"]["content"].as_array().into_iter().flatten() {
                if c["type"] == "tool_use" {
                    u.tool_calls += 1;
                    let name = c["name"].as_str().unwrap_or("?").to_owned();
                    *u.tools.entry(name).or_default() += 1;
                }
            }
        }
        Some("user") => {
            for c in e["message"]["content"].as_array().into_iter().flatten() {
                for part in c["content"].as_array().into_iter().flatten() {
                    if part["type"] == "image" {
                        u.images += 1;
                    }
                }
            }
        }
        Some("result") => {
            u.cost_usd = e["total_cost_usd"].as_f64().unwrap_or(0.0);
            u.turns = n(&e["num_turns"]);
            u.duration_ms = n(&e["duration_ms"]);
            u.result = e["result"].as_str().unwrap_or_default().to_owned();
            let usage = &e["usage"];
            u.input_tokens = n(&usage["input_tokens"]);
            u.cache_creation_tokens = n(&usage["cache_creation_input_tokens"]);
            u.cache_read_tokens = n(&usage["cache_read_input_tokens"]);
            u.output_tokens = n(&usage["output_tokens"]);
            if e["is_error"].as_bool() == Some(true) {
                u.error = Some(format!(
                    "{}: {}",
                    e["subtype"].as_str().unwrap_or("error"),
                    e["terminal_reason"].as_str().unwrap_or("")
                ));
            }
        }
        _ => {}
    }
}

/// The last `VERDICT:` or `RESULT:` line of the answer: `Some(true)` for PASS / FIXED.
pub fn answer(kind: Kind, text: &str) -> Option<bool> {
    let (key, yes, no) = match kind {
        Kind::Verify => ("VERDICT:", "PASS", "FAIL"),
        Kind::Fix => ("RESULT:", "FIXED", "NOT FIXED"),
    };
    text.lines().rev().find_map(|l| {
        let rest = l.trim().trim_start_matches(['*', '`']).strip_prefix(key)?;
        let rest = rest.trim().trim_start_matches(['*', '`']).trim();
        if rest.starts_with(no) {
            Some(false)
        } else if rest.starts_with(yes) {
            Some(true)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_last_answer_line() {
        assert_eq!(
            answer(Kind::Verify, "checked\nVERDICT: PASS — works"),
            Some(true)
        );
        assert_eq!(
            answer(Kind::Verify, "VERDICT: PASS\n**VERDICT: FAIL** crash"),
            Some(false)
        );
        assert_eq!(
            answer(Kind::Fix, "`RESULT: NOT FIXED` no device"),
            Some(false)
        );
        assert_eq!(answer(Kind::Fix, "RESULT: FIXED"), Some(true));
        assert_eq!(answer(Kind::Fix, "done"), None);
    }
}
