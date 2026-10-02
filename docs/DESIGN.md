# mobile-dev-harness Design Overview

> Give coding agents the same edit → run → verify loop for mobile development that they already have on the web.
> Project `mobile-dev-harness`, CLI `mdh`, license MIT OR Apache-2.0.

## Documents

| Document | Contents |
|---|---|
| [design/01-functional.md](design/01-functional.md) | Functional design: users and scenarios, core concepts, feature modules (F1–F10), CLI/MCP/config/flow/output interfaces, non-functional requirements |
| [design/02-architecture.md](design/02-architecture.md) | Technical architecture: crates, core abstractions, key flows, UI tree compression, stability, logs, Gradle probing, helper, MCP, plugin, error model, testing, release, security |
| [adr/](adr/) | Architecture decision records |

## Positioning

- **Not** another device-control MCP. mobile-mcp already covers that layer broadly (iOS, Android, real devices,
  cloud). We don't compete on breadth.
- **A verification harness for agents**: build, state setup, observation, assertions, replay and cost control, end to end.
- The core is agent-agnostic (CLI + MCP); the Claude Code plugin is the first and best-supported integration.
- M1 wins on observation quality and round trips on Android; M2–M4 add the verification loop that device-control
  tools don't cover at all (build diagnostics, state setup, evidence-backed verdicts, replayable flows).

## The seven problems

| # | Problem | Features | Main crate |
|---|---|---|---|
| 1 | Build & install | F2 | `mdh-build` |
| 2 | Verification protocol | F6, F9.2 | `mdh-verify`, plugin |
| 3 | State setup | F3 | `mdh-state` |
| 4 | Logs & crashes | F4.6–F4.8 | `mdh-observe` |
| 5 | Repeatable regression | F7 | `mdh-verify`, `mdh-engine` |
| 6 | Cost & speed | F4.1–F4.4, F8, helper | `mdh-ui`, `android-helper` |
| 7 | Framework differences | F10 | `mdh-build` adapters, `mdh-driver` |

## Roadmap

| Milestone | Features | Done when |
|---|---|---|
| **M0 Scaffold** ✅ | Workspace, CI, licenses, F1.1 `doctor`, F1.2 `devices` | CI green |
| **M1 Observe & act** (adb backend) | `mdh-engine`/Session, ADR-0005 output envelope, F1.3–F1.4, F4.1–F4.8, F5.1–F5.5 (F5.3 via the minimal input helper), F8.1–F8.3, F9.1 (core MCP tools), first `examples/android-sample` | Head-to-head benchmark against mobile-mcp on the sample app (same task set): fewer round trips and tokens at equal or better success rate; results published |
| **M2 Build** | F2.1–F2.6 | Sample app runs with a single `mdh run`; compiler errors come back as structured diagnostics |
| **M3 State & config** | `mdh init`, `mdh.yaml`, F3.1–F3.8, F1.5 | Reach "logged in + specific screen" without any manual tapping |
| **M4 Verify & flows** | F6.1–F6.3, F7.1–F7.4, F9.2 Claude Code plugin, F9.3 | Agents produce evidence-backed verdicts; recorded flows replay in CI → **release 0.1.0** |
| **M5 Helper & performance** | Full `android-helper` (fast UI tree, idle waits, events), F8.4, benchmark | Observation < 300 ms; benchmark results in README → release 0.2.0 |
| **M6 Frameworks & visuals** | F10.1 RN/Expo, F10.2 Flutter, F6.4 visual regression, F7.5 Maestro import | All three example project types pass scenarios S1–S4 |
| **M7 iOS** | F10.3 | S1–S4 pass on the iOS simulator |

> If M1 measurements show `uiautomator dump` latency or the lack of Unicode input is a blocker, the M5 helper
> moves earlier (see architecture §10 and §15).

## Open questions

| Question | Current leaning | Decide by |
|---|---|---|
| Revisit ADR-0004 (no daemon) | Keep | End of M1, based on measurements |
| YAML parser crate | To be evaluated | M3 |
