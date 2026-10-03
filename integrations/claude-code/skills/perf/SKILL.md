---
name: perf
description: Measure Android app performance with mdh against a baseline — cold and hot start, janky frames, frame time, memory and CPU while a flow runs — and explain regressions with a Perfetto trace. Use when impact lists performance items (startup, list binding, drawing) or the user asks whether something got slower.
---

# Performance checks

```sh
mdh perf startup [APP] [--hot] [--runs 5]   # cold (and hot) start; default app: the session's
mdh perf flow NAME [--runs N]               # frames, memory and CPU while a saved flow runs
mdh perf approve [SCOPE]                    # the latest numbers become the baseline
mdh perf explain TRACE --app PKG [--startup]  # summarize a trace a verdict kept
```

- Numbers are medians over several runs with their noise; a regression has to beat both the noise and a
  minimum change, so a pass is a pass. The first run records the baseline per device: commit
  `.mdh/baselines/perf/`.
- A regression comes with a Perfetto trace summarized to what the main thread did. If the verdict says the trace
  processor is missing, **ask the user** before `mdh perf setup --yes` downloads it (about 14 MB).
- Approve only when the slower number is intended.
- For a list or animation, measure a flow that scrolls or plays it (save one with the mdh flow tool first).
- Without a shell, the same is the `mdh_perf` tool (`mdh mcp --tools all`).
