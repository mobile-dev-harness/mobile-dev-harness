# mobile-dev-harness Design Overview

> A quality harness for coding agents on mobile: after every change an agent can drive the app, verify it, and
> check its performance, compatibility and UI consistency — the way agents on the web already open a browser and
> check their work.
> Project `mobile-dev-harness`, CLI `mdh`, license MIT OR Apache-2.0.

## Documents

| Document | Contents |
|---|---|
| [design/01-functional.md](design/01-functional.md) | Functional design: users and scenarios, core concepts, feature modules (F1–F13), CLI/MCP/config/flow/output interfaces, non-functional requirements |
| [design/02-architecture.md](design/02-architecture.md) | Technical architecture: crates, core abstractions, key flows, UI tree compression, stability, logs, Gradle probing, helper, the perf/compat/visual domains, MCP, plugin, error model, testing, release, security |
| [adr/](adr/) | Architecture decision records |

## Model (ADR-0009)

What an agent gets, on top of a shared foundation: **control** to drive the app, **verification** to judge it —
an engine with pluggable check kinds — and the **compatibility matrix** to repeat all of it across devices and
configurations.

| Layer | Crate | Question it answers | Features |
|---|---|---|---|
| **Control** | `mdh-control` | Can the agent drive the app reliably? | F1, F3, F5 |
| **Verification engine** | `mdh-verify` | How are checks run, judged and evidenced? Flows, verdicts, evidence, baselines, reports | F6, F7 |
| ↳ Functional checks | `mdh-verify` | Does it do what it should? | F6.1 |
| ↳ UI consistency checks | `mdh-visual` | Does it match its baseline, its design and accessibility rules? | F13 |
| ↳ Performance checks | `mdh-perf` | Is it fast and lean, and did that regress? | F11 |
| **Compatibility matrix** | `mdh-compat` | Does all of that hold on every version, form factor, configuration and vendor? | F12 |

The shared foundation: `mdh-core` (types, errors, config, output contract), `mdh-driver` (device backends and the
on-device helper), `mdh-observe` (UI trees, logs, crashes, screenshots; F4, F8) and `mdh-project` (build adapters;
F2, F10). Entry points: `mdh-cli` and `mdh-mcp`, plus the Claude Code plugin (F9).

## Positioning

- **Control is where device-control tools such as mobile-mcp stop.** We need it — verification drives the app
  through it — and it is better on Android (compact trees, stable refs, diffs, waiting, ~50× faster trees via the
  helper), but we don't compete on breadth there. It gets finished, not polished further.
- **Verification is where agents have nothing today**: evidence-backed verdicts covering behavior, UI and
  performance, repeatable as flows and across a device matrix. That is where the project's value is.
- The core is agent-agnostic (CLI + MCP); the Claude Code plugin is the first and best-supported integration.

## Roadmap

| Milestone | Features | Done when |
|---|---|---|
| **M0 Scaffold** ✅ | Workspace, CI, licenses, F1.1 `doctor`, F1.2 `devices` | CI green |
| **M1 Control** (in progress) | Output envelope ✅, helper ✅ (ADR-0007), observe/screenshot/input/app lifecycle ✅, session engine ✅ (session-stable refs, ref and selector targeting, wait-for-stable, diffs after actions, recording), F4.6–F4.8 logs and crashes ✅, F9.1 MCP server ✅, `examples/android-sample` ✅ | An agent drives every sample-app scenario through MCP alone ✅. (The benchmark against mobile-mcp moved to the backlog.) |
| **M2 Project** ✅ | F2.1–F2.6 | Sample app runs with a single `mdh run`; compiler errors come back as structured diagnostics |
| **M3 State & config** | `mdh init`, `mdh.yaml`, F3.1–F3.8, F1.5 | Reach "logged in + specific screen" without manual tapping |
| **M4 Verification engine + functional checks** | F6.1–F6.4, F7.1–F7.4, F9.2 Claude Code plugin, F9.3 | Evidence-backed verdicts; recorded flows replay in CI → **release 0.1.0** |
| **M5 UI consistency checks v1** | F13.1 baselines (structural + pixel), F13.3 cross-config layout checks on one device, F13.4 rule checks | A layout regression and a missing label in the sample app show up in a flow's verdict with evidence |
| **M6 Performance checks v1** | F11.1–F11.5 | A startup and a jank regression in the sample app show up in a verdict against a baseline → **release 0.2.0** |
| **M7 Compatibility matrix v1** | F12.1–F12.4 (local emulators and physical devices) | One command runs the sample's flows and their checks across a 3×3 matrix and reports per-cell verdicts |
| **M8 Ecosystem** | F10.1 RN/Expo, F10.2 Flutter, F13.2 design-mock comparison, F12.5 cloud and vendor devices, F7.5 Maestro import | — |
| **M9 iOS** | F10.3 | Control and functional checks pass S1–S4 on the iOS simulator |

## Backlog

| Item | Notes |
|---|---|
| **Benchmark against mobile-mcp** | Same task set on `examples/android-sample` with both tools; compare round trips, tokens, latency and success rate; publish in README. Deferred from M1 (2026-10-02). |
| Display size from the helper | `ScreenInfo.size` is derived from window bounds; while a dialog is the only window (crash dialog, permission prompt) it is too small. The helper should report the display's real size. |
| Compose content without semantics | Drawn Compose content (a canvas without semantics) is absent from the accessibility tree, so opaque-region detection can't see it. Candidate: screenshot-based detection of unexplained drawn areas (M5, `mdh-visual`). |
| Calls while the app is frozen | Settling abandons tree reads after 2 s, but the helper keeps serving the blocked request; the next call can wait up to ~10 s. A per-request deadline inside the helper would bound it. |

## Open questions

| Question | Current leaning | Decide by |
|---|---|---|
| Revisit ADR-0004 (no daemon) | Keep | End of M1, based on measurements |
| YAML parser crate | To be evaluated | M3 |
| Pixel comparison method (SSIM vs. perceptual hash vs. per-pixel with tolerance) | Structural diff first, SSIM for pixels | M5 |
| Accessibility rules: own checks vs. Google's Accessibility Test Framework in an optional helper APK | Own checks for the basics | M5 |
| Cloud device providers to support first | Firebase Test Lab | M8 |
