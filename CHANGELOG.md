# Changelog

All notable changes to mobile-dev-harness. Versions follow [semver](https://semver.org) (0.x: anything may
change); JSON output changes are listed here and carry the `schema` version of the output envelope.

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
