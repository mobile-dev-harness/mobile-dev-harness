# ADR-0009: Verification is an engine; checks plug into it; compatibility is a matrix

- Status: Accepted (2026-10-03); compatibility as a matrix superseded by [ADR-0011](0011-risk-driven-compatibility.md)
- Supersedes: the domain structure of ADR-0008 (the shared foundation and the rationale for leaving device
  control behind stay)

## Context
ADR-0008 named five peer domains: control, verify, performance, compatibility and UI consistency. They aren't
peers. UI consistency and performance are verification too — they check different things — and compatibility
isn't a check at all: it runs the same checks on more devices and configurations. "Verify" was carrying two
meanings at once: the framework every check needs (flows, verdicts, evidence, baselines, reports) and one kind of
check (does the app behave correctly).

## Decision
Organize along two axes on top of control: **what is checked** and **where**.

```
control (mdh-control)                drive the app
  └─ verification engine (mdh-verify) flows · check interface · verdicts · evidence · baselines · reports
       ├─ what: check kinds          functional (built into mdh-verify) · UI consistency (mdh-visual) ·
       │                             performance (mdh-perf) · later accessibility, security, power…
       └─ where: one device, or the compatibility matrix (mdh-compat): the same flows and checks
                 across versions, form factors, configurations and vendors
```

- **The engine owns flows.** Every check runs on flows: performance measures one, UI consistency inspects the
  screens it passes, the matrix replays it per cell. Recording and replay live in `mdh-verify`.
- **Check kinds implement one interface** and report findings into one verdict, so a single run can say
  "functional: pass; UI: 2 deviations; performance: cold start +180 ms":

  ```rust
  #[async_trait]
  pub trait Check: Send + Sync {
      fn kind(&self) -> CheckKind;                                  // Functional, Visual, Performance, …
      async fn run(&self, cx: &mut CheckContext<'_>) -> Result<Vec<Finding>>;
  }

  pub struct Finding {
      pub kind: CheckKind,
      pub outcome: Outcome,                                          // Pass, Fail, Warn
      pub message: String,
      pub step: Option<usize>,                                       // where in the flow
      pub evidence: Vec<Evidence>,                                   // screenshots, tree excerpts, logs, numbers
  }
  ```

  `CheckContext` gives access to the session (control), the flow's current step, the run directory and the
  baseline store.
- **Compatibility is a matrix runner**, not a check kind. A cell's verdict is an ordinary verdict; the matrix
  report aggregates and deduplicates them. Matrix-only concerns (device pool, configuration guards) stay in
  `mdh-compat`.

Dependencies: entry points → `mdh-compat` → `mdh-visual`, `mdh-perf` → `mdh-verify` → `mdh-control` → foundation.
The crate names stay; only the layering and what each crate means change.

## Consequences
- New check kinds get flows, evidence, baselines, reports and matrix runs for free by implementing `Check`.
- M4 becomes "verification engine + functional checks"; M5 and M6 add UI consistency and performance as check
  kinds; M7 adds the matrix.
- User-facing material describes three things — control, verification (with its check kinds) and the
  compatibility matrix — instead of five domains.
