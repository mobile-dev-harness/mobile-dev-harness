# Benchmark

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

- **Verify** tasks: a teammate's uncommitted change and what it should do; the agent answers `VERDICT: PASS` or
  `VERDICT: FAIL` without changing code. They come in pairs: the same description with a correct and a broken
  change, so an agent that always says PASS (or FAIL) scores 50%.
- **Fix** tasks: a bug report against committed code; the agent fixes it and answers `RESULT: FIXED` or
  `RESULT: NOT FIXED`. The grader builds the agent's version, installs it and replays the hidden checks.

`mdh-bench list` shows them with what each seeds. `mdh-bench validate` checks the graders before anything is
spent: each broken change and seeded bug fails its checks, each correct change and reference fix passes them.

## Isolation

Each run starts from a fresh copy of the sample app in its own git repository (the task's bug committed, the
change to verify left uncommitted), without build output, saved flows or baselines, so no setup knows the
expected behavior in advance. Before each run the emulator is reset: the app uninstalled, animations, rotation,
display size, font scale, dark mode and input methods back to their defaults. Runs go one at a time; repetitions
are the outer loop, so drift over hours spreads evenly over the setups.

Every run must find the device the benchmark started on (AVD, API level, display size and density). Agents are
told to leave the device itself alone, and starting emulators or restarting adb is denied; if a run still ends
on another device, or none, it isn't recorded and the benchmark stops until the device is restored. (An agent
once replaced a crashed emulator with another AVD, and every later run, and the grader, saw a smaller screen.)
Setup D loads the Claude Code plugin from a copy taken when the run started (`<out>/plugin`), so edits to the
repository mid-run don't split its runs across versions. The report names the device.

## Metrics

- **Correct**: verify tasks judged right; fix tasks fixed and said so.
- **False pass**: said it works (or is fixed) when it doesn't, of the runs where it doesn't.
- **False fail**: said it doesn't work (or isn't fixed) when it does, of the runs where it does. An agent without a
  device that fixed the bug but couldn't check it says NOT FIXED and lands here.
- **Fixed**: fix tasks whose hidden checks pass, whatever the agent said.
- Cost (USD, at list price), tokens (input, cache and output), tool calls, screenshots looked at, wall time:
  medians per run, from the agent's event stream.

## Running it

```sh
cargo build --release --bin mdh --bin mdh-bench
./target/release/mdh-bench validate
./target/release/mdh-bench run --out bench/out/<name> --reps 5 --budget 100
./target/release/mdh-bench report bench/out/<name>
```

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
run resumes. Each run keeps its workspace, the agent's event stream (`agent.jsonl`) and the grader's output under
`bench/out/<name>/<task>/<setup>-<n>/`.

## Limits

- One app, one emulator, one agent and model per run: the numbers say how these setups compare here,
  not how any agent does on any app.
- The grader replays mdh flows; each check was validated against the seeded bug and the reference fix, but a fix
  that works differently from the reference could fail a check written for it. Failed fix runs are reviewed by
  hand before results are published.
- Ten tasks are few; per-task outcomes are published next to the totals so a single task's influence is visible.

## Results

In [`results/`](results/), one file per run, with the date, model and versions.
