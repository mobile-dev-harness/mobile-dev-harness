# ADR-0005: Uniform JSON output envelope and error codes

- Status: Accepted (2026-10-02)

## Decision
All `--json` output and MCP structured results share one envelope:

```json
{ "schema": "mdh/v1", "ok": true, "data": {}, "error": null, "warnings": [], "timing_ms": {} }
```

- `error` = `{ code, message, hint, details }`; `code` is a stable UPPER_SNAKE_CASE string and `hint` is mandatory.
- Exit codes: 0 success, 1 verification failed, 2 usage error, 3 environment problem, 4 build failed, 5 app crashed,
  10 internal error.

## Rationale
- Agents learn one parsing shape; error codes are machine-checkable and hints tell the agent what to do next.
- The `schema` version makes future breaking changes detectable.

## Consequences
- **`mdh devices --json` from M0 outputs a bare array and must switch to the envelope at the start of M1.** Changing
  it before 0.1 carries no compatibility cost.
