# Benchmark

Version 2 is being built: its design, the levels and the move to Now in Android are in [DESIGN.md](DESIGN.md).
This page describes how the benchmark runs.

Does an agent verify Android changes more precisely with mdh? The benchmark measures it: the same agent,
model and prompt on seeded-bug tasks in the sample app, in four setups, graded against ground truth.

## Setups

| Setup | What the agent has |
|---|---|
| agent alone | The project, a shell and Gradle; no device (adb is off `PATH` and denied) |
| agent + adb | The same, plus a running emulator and adb: `input`, `uiautomator dump`, `screencap` (it can look at screenshots) |
| agent + mobile-mcp | The same as adb, plus [mobile-mcp](https://github.com/mobile-next/mobile-mcp) |
| agent + mdh | The same as adb, plus mdh's MCP server and its Claude Code plugin (skills, hooks) |

Every setup gets the same system prompt apart from one paragraph describing its tools (`crates/mdh-bench/src/setup.rs`).
The agent is Claude Code in headless mode (`claude -p`), with no user or project settings, only the setup's MCP
servers, web tools disabled and permissions bypassed.

## Tasks

`bench/tasks/<id>/task.yaml`, each with hidden checks in `checks/` (mdh flows the agent never sees):

- **Verify** tasks: a teammate's uncommitted change and what it should do; the agent answers `VERDICT: PASS`,
  `VERDICT: FAIL` or `VERDICT: UNVERIFIED` without changing code. They come in pairs: the same description with a correct and a broken
  change, so an agent that always says PASS (or FAIL) scores 50%.
- **Fix** tasks: a bug report against committed code; the agent fixes it and answers `RESULT: FIXED`,
  `RESULT: NOT FIXED` or `RESULT: UNVERIFIED` (changed the code but couldn't check it). The grader builds the agent's version, installs it and replays the hidden checks.

What a flow can't assert (the status bar's appearance) is a `probes:` entry in `task.yaml`: an `adb shell` command
run right after one of the checks, and the text its output must or must not contain.

Every check starts from cleared app data and gives the first screen 60 seconds, far more than a cold start
takes on the emulator (about two seconds): a slow start must not read as a broken app.

`mdh-bench list` shows them with what each seeds. `mdh-bench validate` checks the graders before anything is
spent: each broken change and seeded bug fails its checks, each correct change and reference fix passes them.

## Isolation

Tasks run on an app from [`apps.yaml`](apps.yaml): the sample app, or Now in Android at a pinned commit (checked out
once under `~/.cache/mdh-bench`). Each run starts from a fresh copy of the app in its own git repository (the task's bug committed, the
change to verify left uncommitted), without build output, saved flows or baselines, so no setup knows the
expected behavior in advance. Before each run the emulator is reset: the app uninstalled, and with it what an
agent's build of the project left there (the test APKs of its modules), animations, rotation, display size, font
scale, dark mode and input methods back to their defaults, the time zone set to America/Los_Angeles. Runs go one at a time; repetitions
are the outer loop, so drift over hours spreads evenly over the setups.

Every run must find the device the benchmark started on (AVD, API level, display size and density). Agents are
told to leave the device itself alone apart from the display settings a check needs (dark theme, font scale, time
zone, rotation, display size; the reset puts them back), and starting emulators or restarting adb is denied; if a run still ends
on another device, or none, it isn't recorded and the benchmark stops until the device is restored. (An agent
once replaced a crashed emulator with another AVD, and every later run, and the grader, saw a smaller screen.)
Setup D loads the Claude Code plugin from a copy taken when the run started (`<out>/plugin`), so edits to the
repository mid-run don't split its runs across versions. The report names the device.

## Metrics

- **Correct**: verify tasks judged right; fix tasks fixed and said so.
- **False pass**: said it works (or is fixed) when it doesn't, of the runs where it doesn't.
- **Claim precision**: of the PASS / FIXED answers, the share that are true.
- **False fail**: said it doesn't work (or isn't fixed) when it does, of the runs where it does.
- **Abstained**: answered UNVERIFIED; never correct, never false.
- **Resolved**: fix tasks whose hidden checks pass, whatever the agent said.
- **Unchecked claims**: PASS / FIXED answers from runs that didn't install the app after their last change to
  the code.
- **Grader errors** (the grader couldn't read the screen or lost the device, twice) are listed and left out.
- Cost (USD, at list price), tokens (input, cache and output), tool calls, screenshots looked at, wall time:
  medians per run, from the agent's event stream.

## Running it

```sh
cargo build --release --bin mdh --bin mdh-bench
./target/release/mdh-bench validate
./target/release/mdh-bench run --out bench/out/<name> --reps 5 --budget 100
./target/release/mdh-bench report bench/out/<name>
```

`mdh-bench regrade bench/out/<name>` grades recorded fix runs again from their kept workspaces (after a grader
fix) and re-reads every transcript; `--failed-only` limits it to runs graded as not working or as grader errors,
`--transcripts-only` needs no device. `mdh-bench review --tasks <id>` has GLM review a task against the checklist
in DESIGN.md (3.4) and writes `bench/tasks/<id>/review.md`.

Other models: any provider with an Anthropic-compatible endpoint for Claude Code. Put the variables its
documentation gives for Claude Code in `~/.config/mdh-bench/<name>.env` (`chmod 600`; never in the repository),
plus `MODEL=<model id>`:

```sh
ANTHROPIC_BASE_URL=https://…
ANTHROPIC_AUTH_TOKEN=…
MODEL=…
```

then `mdh-bench probe --provider <name>` checks that it answers, calls tools and reads images, and
`mdh-bench run --provider <name> …` runs with it. Claude Code doesn't know other providers' prices, so the report
leaves their cost out and `--budget` can't stop such a run: mind the subscription's quota.

One emulator must be online. `run` appends to `results.jsonl` and skips runs already recorded, so an interrupted
run resumes. Each run keeps its workspace (without build output: regrading builds it again), the agent's event
stream (`agent.jsonl`) and the grader's output under `bench/out/<name>/<task>/<setup>-<n>/`. The agent alone on a
verify task never touches the emulator, so those runs neither reset nor check it and can go on while it is in use.

## Limits

- One app, one emulator, one agent and model per run: the numbers say how these setups compare here,
  not how any agent does on any app.
- The grader replays mdh flows; each check was validated against the seeded bug and the reference fix, but a fix
  that works differently from the reference could fail a check written for it. Failed fix runs are reviewed by
  hand before results are published.
- Ten tasks are few; per-task outcomes are published next to the totals so a single task's influence is visible.

## Results

In [`results/`](results/), one file per run, with the date, model and versions.
