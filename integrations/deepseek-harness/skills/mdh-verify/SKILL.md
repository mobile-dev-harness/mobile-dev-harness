---
name: mdh-verify
description: Verify an Android app change with mdh by finding affected screens, building and running the app, checking behavior, and replaying relevant flows. Use after changes to app code, resources or the manifest, or when asked to verify a fix on a device.
---

# Verify an Android change

DSH prefixes MCP tool names with `mcp__mdh__` by default. The examples name the
underlying tool; resolve its actual discovered name if the server was customized.

The MCP process must run in the intended Android project. Its working directory
owns flows, baselines, evidence and changed-flow selection; a per-call project
argument does not rebind it. Keep one device workflow at a time. Examples below
use illustrative targets and expectations; replace them with the actual app.

Inspect impact, then build and run. Fix broken call sites and structured build
errors before checking screens. Impact is syntax-based guidance, not complete coverage.

```mcp
{"tool":"mdh_impact","arguments":{}}
```

```mcp
{"tool":"mdh_run","arguments":{}}
```

If needed, select the device the user chose. Start an emulator only with the user's
authorization; preserve prior authorization instead of asking again. These are
alternative actions, not instructions to switch away from the active device.

```mcp
{"tool":"mdh_status","arguments":{"device":"emulator-5554"}}
```

```mcp
{"tool":"mdh_status","arguments":{"start_emulator":"Pixel_API_36"}}
```

Visit affected screens using a known deep link or observed targets. Use current
refs, selectors or labels, never guessed coordinates or refs from another session.
Inspect a screenshot for images, charts and opaque regions the tree cannot explain.

```mcp
{"tool":"mdh_app","arguments":{"command":"open","app":"myapp://settings"}}
```

```mcp
{"tool":"mdh_observe","arguments":{"screenshot":true}}
```

```mcp
{"tool":"mdh_act","arguments":{"actions":[{"action":"type","into":"id=email","text":"demo@example.com"},{"action":"tap","target":"Sign in"},{"action":"wait","target":"id=title","timeout_s":10}]}}
```

Check the requested behavior. The verdict always includes `no crash`; inspect
failed observations and evidence before fixing and repeating the affected checks.

```mcp
{"tool":"mdh_verify","arguments":{"checks":["screen .LoginActivity","enabled id=sign_in"],"timeout_s":3}}
```

Replay flows selected by the uncommitted change, or explicitly selected flows.
Missing flows are a coverage gap. Do not combine these alternative selection modes.

```mcp
{"tool":"mdh_verify","arguments":{"changed":true}}
```

```mcp
{"tool":"mdh_verify","arguments":{"flows":["login-success"]}}
```

Save a useful recording with its final assertions. Review setup and supply any
required environment variables before replay; passwords may be recorded as env references.

```mcp
{"tool":"mdh_flow","arguments":{"command":"save","name":"login-success","checks":["text id=title == \"Inbox\""]}}
```

The default connection exposes eight core tools. Use the shell's `mdh visual`,
`mdh perf` or `mdh compat` for specialist checks. Explicitly opt in to the 11-tool
`tools: all` set for a client without a shell, or when specialist checks should
share the active MCP session; preserve the remaining profile config.
CLI state in `.mdh/session.json` is separate: use the intended project directory,
explicit device and app, and establish its own screen or saved-flow setup.

Report verdicts, evidence paths and unverified conditions. A build or old passing
verdict does not prove the current change. After a crash, preserve the recording
and use saved-flow replay's fresh window or reset before repeating the full path;
rebuilding alone does not clear the session's crash window.
