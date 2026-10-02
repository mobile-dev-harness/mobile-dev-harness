# ADR-0004: No resident daemon in the first release

- Status: Accepted (2026-10-02)

## Context
In CLI mode every invocation is a new process, yet session state (ref table, previous UI tree, log cursor,
recording) must persist across invocations.

## Decision
- MCP mode: the session lives in memory in the MCP server process.
- CLI mode: the session is serialized to `.mdh/session.json`, loaded on each invocation and written back at the end.
- No background daemon process.

## Rationale
- A daemon is expensive in complexity: lifecycle management, IPC, version skew, zombie processes, several projects
  at once.
- First-release latency is dominated by `uiautomator dump` and Gradle, not by process startup or state loading;
  Rust process startup is negligible.
- Logs are read incrementally with a timestamp cursor, so no resident logcat process is needed.

## Revisit when
Real-time crash push, a pooled helper connection, or CLI-mode latency becomes a requirement.
