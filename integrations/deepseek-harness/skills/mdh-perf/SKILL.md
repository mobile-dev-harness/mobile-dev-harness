---
name: mdh-perf
description: Measure Android startup or saved-flow performance with mdh, compare repeated measurements with per-device baselines, and explain regressions with Perfetto. Use for startup, frame, memory or CPU regressions and performance-sensitive app changes.
---

# Measure Android performance

Use the shell for these specialist checks; the default MCP connection exposes
only eight core tools. Work in the intended project and select an explicit device
and package. CLI state is separate from MCP: a flow needs a saved file with the
correct app and setup, not MCP-only recorded actions. Replace these example values.

```sh
cd /absolute/path/to/android-project
mdh --device SERIAL perf startup com.example.app --hot --runs 5
mdh --device SERIAL perf flow scroll-list --runs 5
mdh perf explain /absolute/path/to/trace.perfetto --app com.example.app --startup
```

Measure a flow that exercises the interaction at issue. Use repeated measurements:
mdh compares medians with noise and minimum-change thresholds. Compare the same
device profile. The first run creates `.mdh/baselines/perf/`; it is not proof of
no regression. Report measured values and the actual verdict's scope.

Approve a slower result only when intended and accepted, using a specific scope:

```sh
mdh perf approve startup-com.example.app
```

If explanation needs the trace processor, download it only with the user's
authorization. Existing authorization remains valid; do not ask again for the
same download. Without it, report that explanation was not performed.

```sh
mdh perf setup --yes
```

Explicitly opt in to `tools: all` for a client without a shell, or when specialist
checks should share the active MCP session; preserve the remaining profile config.
DSH prefixes MCP tool names with `mcp__mdh__` by default. The examples name the
underlying tool; resolve its actual discovered name if the server was customized.
These are the corresponding MCP invocation shapes; use only the operation needed:

```mcp
{"tool":"mdh_perf","arguments":{"command":"startup","app":"com.example.app","hot":true,"runs":5}}
```

```mcp
{"tool":"mdh_perf","arguments":{"command":"flow","flow":"scroll-list","runs":5}}
```

```mcp
{"tool":"mdh_perf","arguments":{"command":"explain","path":"/absolute/path/to/trace.perfetto","app":"com.example.app","startup":true}}
```

```mcp
{"tool":"mdh_perf","arguments":{"command":"approve","scope":"startup-com.example.app"}}
```

The setup invocation also requires the download authorization described above:

```mcp
{"tool":"mdh_perf","arguments":{"command":"setup","consent":true}}
```

Use the cold-start option only for a cold-start trace. A compatibility run does
not automatically execute a flow's `perf:` section; measure it separately.
