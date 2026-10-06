---
name: mdh-compat
description: Verify Android compatibility risks identified by mdh using targeted device and configuration runs. Use for SDK checks, target SDK changes, qualified resources, rotation, saved state, background behavior, or impact-reported compatibility risks.
---

# Verify Android compatibility risks

Use the shell for these specialist checks; the default MCP connection exposes
only eight core tools. Work in the intended project directory and select the
device explicitly. Confirm saved flows name the intended app and setup. CLI and
MCP sessions are separate: save any MCP-only recording before CLI replay.

```sh
cd /absolute/path/to/android-project
mdh compat risks --project . --base HEAD
mdh --device SERIAL compat plan --project . --base HEAD --max-emulators 1
mdh --device SERIAL compat run --project . --base HEAD --no-start --max-emulators 1
```

Inspect source evidence and the planned cells before running. Use the same base
revision throughout. Leave new emulators unstarted unless authorized. Once the
user authorizes the required starts, replace `--no-start` with `--yes`; preserve
existing authorization and any requested emulator limit.

Explicitly opt in to `tools: all` for a client without a shell, or when specialist
checks should share the active MCP session; preserve the remaining profile config.
DSH prefixes MCP tool names with `mcp__mdh__` by default. The examples name the
underlying tool; resolve its actual discovered name if the server was customized.
The equivalent MCP alternatives are:

```mcp
{"tool":"mdh_compat","arguments":{"command":"risks","project":".","base":"HEAD"}}
```

```mcp
{"tool":"mdh_compat","arguments":{"command":"plan","project":".","base":"HEAD","max_emulators":1}}
```

```mcp
{"tool":"mdh_compat","arguments":{"command":"run","project":".","base":"HEAD","no_start":true,"max_emulators":1}}
```

Only after authorization for the required emulator starts, use this alternative:

```mcp
{"tool":"mdh_compat","arguments":{"command":"run","project":".","base":"HEAD","consent":true,"max_emulators":1}}
```

The MCP server's working directory still owns flows and evidence; a per-call
project argument does not rebind it. Missing vendor devices, missing flows and
omitted cells remain unverified. Save a suitable reproduction with `mdh_flow`
when needed, then rerun. Report failed, passed and unverified risks separately.

This risk-driven selection is not exhaustive device coverage. Compatibility does
not automatically apply flows' `visual:` or `perf:` sections; run relevant checks
separately. Device settings should be restored on every path. After interruption,
inspect display and rotation settings before reusing the device.
