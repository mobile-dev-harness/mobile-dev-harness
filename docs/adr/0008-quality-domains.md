# ADR-0008: Organize around five quality domains on a shared foundation

- Status: Accepted (2026-10-02); domain structure superseded by [ADR-0009](0009-verification-engine.md)
- Amends: positioning and roadmap in DESIGN.md; crate layout in architecture §1–2

## Context
The design so far was organized around one loop — build, run, verify — with crates split by technical layer
(`mdh-ui`, `mdh-state`, `mdh-build`, …). Agents need more than functional verification after a change: whether the
app got slower, whether it still works on other Android versions, screen sizes, configurations and vendors, and
whether the UI still matches its baseline, its design and basic accessibility rules. None of this is covered by
device-control tools such as mobile-mcp, which only cover control.

## Decision
Organize the product and the code around five quality domains built on a shared foundation:

| Domain | Crate | Question it answers |
|---|---|---|
| Control | `mdh-control` | Can the agent drive the app reliably? (sessions, targeting, actions, waiting, state, navigation) |
| Verify | `mdh-verify` | Does it do what it should? (assertions, verdicts, flows, evidence) |
| Performance | `mdh-perf` | Is it fast and lean, and did that regress? (startup, rendering, memory, CPU, budgets) |
| Compatibility | `mdh-compat` | Does it work everywhere it should? (versions, form factors, configurations, vendors) |
| UI consistency | `mdh-visual` | Does it look the way it should? (baselines, design mocks, cross-config layout, rules) |

Foundation crates: `mdh-core` (types, errors, config, output contract), `mdh-driver` (device backends),
`mdh-observe` (UI trees, logs, crashes, screenshots; formerly `mdh-ui`), `mdh-project` (build adapters).
Entry points: `mdh-cli`, `mdh-mcp`.

Dependencies point downward: entry points → verify / perf / compat / visual → control → observe / project → driver
→ core. Compat is an orchestrator: it runs verify, perf and visual checks across a device matrix, so it may depend
on all three. Perf and visual expose their checks as assertion kinds that verify can evaluate.

Domain crates exist from the start with their scope documented, and stay empty until their milestone.

## Rationale
- Control is the layer mobile-mcp already covers. It stays necessary — every other domain drives the app through it
  — but it is no longer where the project competes; it gets finished, not polished further.
- Verify, performance, compatibility and UI consistency are where agents currently have nothing. Naming them as
  first-class domains keeps the roadmap honest about where the value is.
- Mapping domains to crates gives contributors an obvious home for each feature and keeps domain logic out of the
  entry points.

## Consequences
- Scope grows considerably; each domain's first version must stay narrow (see the roadmap).
- Vendor coverage and design-mock comparison need resources outside a local machine (physical or cloud devices, the
  Figma API) and are scheduled late.
- Emulator performance numbers are not representative in absolute terms; perf reports compare against baselines on
  the same device and include variance.
