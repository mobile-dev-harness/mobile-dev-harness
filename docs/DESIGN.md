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

## Five quality domains (ADR-0008)

| Domain | Crate | Question it answers | Features |
|---|---|---|---|
| **Control** | `mdh-control` | Can the agent drive the app reliably? | F1, F3, F5 |
| **Verify** | `mdh-verify` | Does it do what it should? | F6, F7 |
| **Performance** | `mdh-perf` | Is it fast and lean, and did that regress? | F11 |
| **Compatibility** | `mdh-compat` | Does it work on every version, form factor, configuration and vendor it should? | F12 |
| **UI consistency** | `mdh-visual` | Does it match its baseline, its design and basic accessibility rules? | F13 |

They share a foundation: `mdh-core` (types, errors, config, output contract), `mdh-driver` (device backends and the
on-device helper), `mdh-observe` (UI trees, logs, crashes, screenshots; F4, F8) and `mdh-project` (build adapters;
F2, F10). Entry points: `mdh-cli` and `mdh-mcp`, plus the Claude Code plugin (F9).

## Positioning

- **Control is where device-control tools such as mobile-mcp stop.** We need it — every other domain drives the
  app through it — and it is better on Android (compact trees, stable refs, diffs, waiting, ~50× faster trees via
  the helper), but we don't compete on breadth there. It gets finished, not polished further.
- **Verify, performance, compatibility and UI consistency are where agents have nothing today.** That is where the
  project's value is.
- The core is agent-agnostic (CLI + MCP); the Claude Code plugin is the first and best-supported integration.

## Roadmap

| Milestone | Features | Done when |
|---|---|---|
| **M0 Scaffold** ✅ | Workspace, CI, licenses, F1.1 `doctor`, F1.2 `devices` | CI green |
| **M1 Control** (in progress) | Output envelope ✅, helper ✅ (ADR-0007), observe/screenshot/input/app lifecycle ✅, session engine ✅ (session-stable refs, ref and selector targeting, wait-for-stable, diffs after actions, recording), F4.6–F4.8 logs and crashes ✅, F9.1 MCP server ✅; first `examples/android-sample` | Head-to-head benchmark against mobile-mcp on the sample app (same task set): fewer round trips and tokens at equal or better success rate; results published |
| **M2 Project** | F2.1–F2.6 | Sample app runs with a single `mdh run`; compiler errors come back as structured diagnostics |
| **M3 State & config** | `mdh init`, `mdh.yaml`, F3.1–F3.8, F1.5 | Reach "logged in + specific screen" without manual tapping |
| **M4 Verify** | F6.1–F6.3, F7.1–F7.4, F9.2 Claude Code plugin, F9.3 | Evidence-backed verdicts; recorded flows replay in CI → **release 0.1.0** |
| **M5 UI consistency v1** | F13.1 baselines (structural + pixel), F13.3 cross-config layout checks on one device, F13.4 rule checks | A layout regression and a missing label in the sample app are caught with evidence |
| **M6 Performance v1** | F11.1–F11.5 | A startup and a jank regression in the sample app are caught against a baseline → **release 0.2.0** |
| **M7 Compatibility v1** | F12.1–F12.4 (local emulators and physical devices) | One command runs the sample's flows across a 3×3 matrix and reports per-cell results |
| **M8 Ecosystem** | F10.1 RN/Expo, F10.2 Flutter, F13.2 design-mock comparison, F12.5 cloud and vendor devices, F7.5 Maestro import | — |
| **M9 iOS** | F10.3 | Control and verify pass S1–S4 on the iOS simulator |

## Open questions

| Question | Current leaning | Decide by |
|---|---|---|
| Revisit ADR-0004 (no daemon) | Keep | End of M1, based on measurements |
| YAML parser crate | To be evaluated | M3 |
| Pixel comparison method (SSIM vs. perceptual hash vs. per-pixel with tolerance) | Structural diff first, SSIM for pixels | M5 |
| Accessibility rules: own checks vs. Google's Accessibility Test Framework in an optional helper APK | Own checks for the basics | M5 |
| Cloud device providers to support first | Firebase Test Lab | M8 |
