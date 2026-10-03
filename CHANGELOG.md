# Changelog

All notable changes to mobile-dev-harness. Versions follow [semver](https://semver.org) (0.x: anything may
change); JSON output changes are listed here and carry the `schema` version of the output envelope.

## Unreleased

### Performance checks
- `mdh perf startup [APP] [--hot]`: cold (and hot) start over repeated runs; `mdh perf flow NAME`: janky frames,
  p90 and p99 frame time, memory (PSS) and CPU while a saved flow runs. Median, p90 and noise (MAD) per metric; a
  first run is discarded and animations stay on.
- Baselines per device profile (AVD or model, API level, debug or release) in `.mdh/baselines/perf/`, recorded by
  the first measurement and promoted by `mdh perf approve`; a regression must exceed three times the noise and a
  per-metric minimum. Budgets in a flow's `perf:` section. Memory that grows with every run is reported.
- Regressions are explained by a Perfetto trace of one more run, summarized to the main-thread work behind it
  (startup, late frames, busiest work, GC); the trace is kept for ui.perfetto.dev and `mdh perf explain`.
  Perfetto's trace processor (pinned v58.2, hash-checked) is downloaded only after the user agreed
  (`mdh perf setup`); the new error code `NEEDS_CONSENT` says so to agents.
- MCP: `mdh_perf`.

### UI consistency checks
- `mdh visual check`: rules on the compact tree (touch targets, labels, overlap, controls under system bars,
  duplicate labels) and text contrast on pixels; structural (dp) and pixel baselines per device profile, approved
  with `mdh visual approve`; the same screen at font scale 1.3, in dark mode and right to left (`--configs`).
- Flows run them at every checkpoint, configured by their `visual:` section. MCP: `mdh_visual`.

### Devices
- `mdh devices` lists startable emulators; `mdh devices use` sets the project's default device;
  `mdh emulator start|stop`. With nothing online, mdh asks before starting an emulator (agents get the options in
  the error and ask their user).

## 0.1.0

The first release: precise verification for coding agents on Android.

### Control
- Compact screens (~150 tokens) with session-stable refs, actions (`tap`, `long-press`, `type` with any Unicode,
  `swipe`, `scroll --until`, `key`, `wait`) that wait for the UI to settle and report only what changed, obscured
  elements and system overlays.
- Crashes, native crashes and ANRs with every result, de-noised stack traces and the steps that led there.
- An on-device helper reads the UI in ~10 ms; uiautomator remains the fallback.
- `mdh run`: Gradle probing, builds with `file:line` diagnostics, install-if-changed (the right ABI split),
  restart and the first screen.
- State: animations off and back, runtime permissions, data reset, deep links (`mdh state`, `mdh open`).

### Verification
- `mdh verify`: checks on the screen and the logs (`visible`, `not visible`, `enabled`, `checked`, `text`,
  `screen`, `log`, always `no crash`) in one verdict with what was observed and evidence on disk.
- Flows: the session's recording saved as YAML (`mdh flow save`), replayed from a clean start with setup
  (`mdh flow run`), JUnit reports.
- `mdh impact`: the screens a code change reaches, how to reach them, broken call sites and what to verify,
  from tree-sitter analysis of Kotlin, Java and Android XML in ~150 ms, no device needed; `mdh flow run --changed`
  replays the flows the change needs.

### Integration
- MCP server (`mdh mcp`) with the same engine: `mdh_run`, `mdh_observe`, `mdh_act`, `mdh_wait`, `mdh_logs`,
  `mdh_app`, `mdh_verify`, `mdh_flow`, `mdh_impact`, `mdh_status`.
- Claude Code plugin with `verify` and `debug-crash` skills and hooks; `mdh init` adds a "how to verify" section
  to `AGENTS.md` for other agents.
- Output: text for agents and people, the `mdh/v1` JSON envelope with stable error codes and exit codes.
