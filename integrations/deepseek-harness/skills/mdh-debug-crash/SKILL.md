---
name: mdh-debug-crash
description: Diagnose an Android crash, native crash or ANR reported by mdh, reproduce it, and verify a scoped fix. Use for APP_CRASHED, mdh crash reports, or a failing no-crash check.
---

# Debug an Android crash

DSH prefixes MCP tool names with `mcp__mdh__` by default. The examples name the
underlying tool; resolve its actual discovered name if the server was customized.

Use the connection for the affected project and device. Read the existing report:
exception, nested causes, app frames and preceding actions. Request surrounding
logs when useful; reading a report does not erase its crash from the session.

```mcp
{"tool":"mdh_observe","arguments":{"logs":"info","lines":100}}
```

Trace the root cause into app code. For an ANR, inspect main-thread I/O, locks and
long work on the triggering path. For a native signal, inspect app library frames
and the Java/Kotlin entry into native code. Reproduce the report's actions with
`mdh_act`, or replay the failing saved flow. Preserve the original evidence.

If no reusable flow exists, save the recorded reproduction before any reset,
with final assertions for the repaired behavior. Review before overwriting an
existing flow. Reset discards session refs and recorded steps.

```mcp
{"tool":"mdh_flow","arguments":{"command":"save","name":"login-crash","checks":["text id=title == \"Inbox\""]}}
```

Make a scoped fix and rebuild. Prefer replaying the saved flow, which gives
`no crash` a fresh verification window while exercising the same path.

```mcp
{"tool":"mdh_run","arguments":{}}
```

```mcp
{"tool":"mdh_verify","arguments":{"flows":["login-crash"]}}
```

`mdh_run` and app launch alone do not clear the original session crash window.
Without a reusable flow, first preserve evidence and any recording, then reset,
rebuild, reproduce the complete path and verify its intended screen assertions.

```mcp
{"tool":"mdh_status","arguments":{"reset":true}}
```

```mcp
{"tool":"mdh_verify","arguments":{"checks":["text id=title == \"Inbox\""]}}
```

Current-screen verification includes crashes already read in that session;
resetting without repeating the path does not prove a fix. Use `mdh_impact` to
select related checks. Report the cause, reproduction verdict and evidence paths.

Honor existing device and emulator authorization. Do not clear data or reinstall
merely to hide the reproduction. A CLI fallback needs the same project directory,
explicit device and app, and its own session setup; it does not inherit MCP state.
