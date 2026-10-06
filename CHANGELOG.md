# Changelog

All notable changes to mobile-dev-harness. Versions follow [semver](https://semver.org) (0.x: anything may
change); JSON output changes are listed here and carry the `schema` version of the output envelope.

## Unreleased

### Flows
- A flow can set the device it needs, before the app starts, and puts it back afterwards: `setup.device` with
  `dark`, `font_scale`, `time_zone`, `locale` (the app's language), `orientation` and `display`
  (`1600x2560@320`). Settings are read back once the app runs; one the device doesn't keep makes the verdict
  ERROR.

### Fixes
- A flow could fail at its first step with the launcher in front, or stop with an ERROR after two minutes, about
  once in 15 runs, when it reset an app that was showing a permission dialog. `pm clear` returns while the app's
  task is still closing, and since Android 14 the end of the dialog kills the app's processes with that task up
  to a second later: the one the flow had just started included, without an error. Clearing an app's data
  (`state clear-data`, a flow's `reset: data`) now returns once the app's tasks are gone: half a second later
  than before when the app was in front, a second when a dialog was.
- A device that already had a newer on-device helper, installed by another mdh (another checkout, a newer
  release), made every command that reads the screen fail with `INSTALL_FAILED_VERSION_DOWNGRADE` until someone
  uninstalled the helper by hand. mdh now uninstalls it and installs its own, as it did for a helper signed with
  another key. Two mdh versions sharing a device replace each other's helper on every switch (about a second). If
  the helper can't be installed, the error is `HELPER_UNAVAILABLE` with Android's reason and what to do (it was a
  bare `COMMAND_FAILED`).
- Controls an app draws entirely under the status bar, the navigation bar or the keyboard are in the tree,
  marked `obscured`, instead of missing: with the window list on, Android reports such a node as not visible to
  the user and the on-device helper skipped it. On Now in Android's topic screen with its top inset lost, the
  Back button and the follow chip were drawn over the clock while the tree started at the title and
  `visible Back` failed with "not on screen". The helper (version 6) keeps what lies under a system window;
  `visible`, taps and the `obscured` rule then say it is covered (the sample's `OverlapActivity`, broken on
  purpose, had come to pass that rule). `not visible` and `wait --gone` no longer take such an element for
  gone. `wait`, a flow step waiting for its target and `scroll --until` wait until part of it is clear (the
  keyboard on its way out, a row scrolling in under a bar) and say that it is covered when it stays so.
- An action could return while the keyboard was still sliding in or out, with `keyboard` and the obscured
  elements out of date: over an app that doesn't resize for the keyboard the tree is the same in every frame.
  Settling now also waits for the windows to stay where they are.
- The screen could be read stale: the on-device helper read through an accessibility cache that a Compose
  navigation inside one window (Now in Android's topic pages) didn't invalidate, so mdh kept showing the
  previous screen while the device showed the new one. The helper (version 5) drops the cache before every
  read.
- A Compose chip that is selectable around its own checkbox (Now in Android's topics) is one element, not two
  with the same name: targeting it by name matched twice.
- A row or card is no longer shown as the toggle inside it when that toggle is clickable itself. A Now in
  Android news card whose only control on screen was its bookmark button read as `checkbox "Bookmark"`, and
  tapping it opened the article; the "Dark theme" row of Settings, which opens a page, read as its switch. They
  are two elements now, the card or row and its toggle. A row still reads as its switch when the switch takes no
  taps itself, as in a preference row. A flow that targets a row with a clickable switch as a switch
  (`role=switch;text=…`) no longer finds it: target the row by its name, or the switch inside it. The sample
  app's settings rows now leave the tap to the row (`android:clickable="false"` on the switch) and still read
  as switches.
- Projects with Isolated Projects on (`org.gradle.isolated-projects=true`, as in Now in Android) can be built
  and run: reading the project's modules and variants turned the configuration cache off, which Gradle refuses
  there. Isolated Projects is now off for that one Gradle call.
- `mdh run` on a project with several application modules lists each module with the variant it would build, or
  with the variants to choose from (`:app (demoDebug or prodDebug), :app-nia-catalog (debug)`), and its hint says
  to pass `--variant` with `--module` where several are listed. It listed only the modules, and the call that
  named one came back with a second `AMBIGUOUS_BUILD_TARGET` for the variant: on Now in Android every benchmark
  run with mdh began with these two failed calls. In JSON output the text of the error's `message` and `hint`
  changes; its code and shape don't.
- Locking the rotation (flows, compatibility cells) could fall back to portrait mid-run: turning auto-rotate off
  made the window manager store the old rotation after mdh had set the new one. It is locked in one step now
  (`cmd window user-rotation`, Android 11+).
- `scroll --until` (and a flow's `scroll: {until: …}`) keeps scrolling while the content moves, up to 50
  scrolls, instead of giving up after 10: on a shorter screen a row near the end of a long list was reported
  missing although it was there.
- A flow step that fails because the check couldn't be made (the screen unreadable, the device or a tool gone, a
  target the flow can't resolve) makes the verdict ERROR instead of FAIL: a broken tool chain no longer reads
  as a broken app. Seen in the benchmark, where the grader couldn't read the screen and two correct fixes were
  judged broken.

### Benchmark
- A run must end on the device the benchmark started on (AVD, API level, display size and density); otherwise
  it isn't recorded and the benchmark stops. Agents are told to leave the device alone, starting emulators and
  restarting adb are denied, and input methods are reset between runs.
- Setup D loads the plugin from a copy taken when the run started; the report names the device; a run that
  timed out says so.

## 0.4.0

Fewer tokens on every request, a benchmark, and a home in the mobile-dev-harness organization.

### Breaking changes (MCP)
- `mdh_wait` is an action of `mdh_act` (`{"action": "wait", "target": …, "gone": …, "timeout_s": …}`) and
  `mdh_logs` an option of `mdh_observe` (`{"logs": "warn", "lines": 50}`).
- An action is one flat object, an `action` kind plus the fields it uses (`target`, `text`, `into`, `key`,
  `direction`, `until`, `from`, `to`, …): scroll's container is `target` (was `within`), a key is `key` (was
  `name`).
- `mdh mcp` offers the core tools by default (8: status, observe, act, app, run, verify, flow, impact).
  `mdh mcp --tools all` adds `mdh_visual`, `mdh_perf` and `mdh_compat` for clients without a shell; agents with a
  shell use `mdh visual|perf|compat`, which the Claude Code plugin's new `visual`, `perf` and `compat` skills
  describe.

### Fewer tokens
- Every tool definition is sent with every request: schemas are compacted when the server starts and
  descriptions are one sentence, guidance having moved into skills loaded only when used. Measured on Claude
  Code's first request, mdh adds 3.2k tokens instead of 7.4k. A test keeps the core definitions under 6,500
  characters.
- The verify skill says to look at a screenshot when images or drawings matter.

### Benchmark
- `mdh-bench`: seeded-bug tasks in the sample app (verify tasks in correct/broken pairs, fix tasks graded by hidden
  mdh flows), run by Claude Code in four setups — alone, with adb, with mobile-mcp, with mdh — on fresh copies of
  the app and a reset emulator, reporting false passes, false fails, fixes, tokens, tool calls, screenshots and
  time. `validate` checks the graders first; `--provider` runs other models through Anthropic-compatible
  endpoints, `probe` checks one. Method in `bench/README.md`.

### Project
- The repository moved to the [mobile-dev-harness](https://github.com/mobile-dev-harness) organization; old links
  redirect.
- The compatibility knowledge base lives in [compat-kb](https://github.com/mobile-dev-harness/compat-kb); mdh
  vendors a pinned release (`scripts/update-kb.sh`), and `MDH_COMPAT_KB` points it at another copy.
- Repositories follow layers (ADR-0012): impact analysis and compatibility risk analysis (`mdh-impact`, the new
  `mdh-risk`) depend on no device crate; `mdh-impact` has its own error type.
- A security policy (private vulnerability reporting), issue forms, a pull request template, and a logo.

## 0.3.0

Compatibility, verified risk by risk from the change (ADR-0011).

### Compatibility
- `mdh compat risks`: compatibility risks of the change, from impact analysis and a knowledge base with sources —
  API-level branches (both sides), behavior changes of the app's target SDK and of newer Android versions,
  `minSdk` and `targetSdk` changes, large-screen and orientation resources, window size classes, folding, saved
  state, cars and TVs, vendor ROMs (background restrictions, autostart, Google Play services, overlays), screen
  sizes. No device.
- `mdh compat plan` / `mdh compat run`: the fewest cells covering the risks — display overrides for a small phone,
  a foldable and a tablet, landscape, on the current emulator; other devices and AVDs by API level and vendor —
  then a verdict per risk: new rule violations compared with the device as it is, state lost across a rotation,
  flows failing only there; unverified risks say what's missing. MCP: `mdh_compat`.
- `mdh impact` lists the compatibility risks; manifest entries show their attributes before and after.

## 0.2.0

Performance and UI consistency checks join functional checks in the verification engine.

### Performance checks
- `mdh perf startup [APP] [--hot]`: cold (and hot) start over repeated runs; `mdh perf flow NAME`: janky frames,
  p90 and p99 frame time, memory (PSS) and CPU while a saved flow runs. Median, p90 and noise (MAD) per metric; a
  first run is discarded and animations stay on.
- Baselines per device profile (AVD or model, API level, debug or release) in `.mdh/baselines/perf/`, recorded by
  the first measurement and promoted by `mdh perf approve`; a regression must exceed three times the noise and a
  per-metric minimum. Budgets in a flow's `perf:` section. Memory that grows with every run is reported.
- Regressions are explained by a Perfetto trace of one more run, summarized to the main-thread work behind it
  (startup, late frames, busiest work, GC); the trace is kept for ui.perfetto.dev and `mdh perf explain`.
  Perfetto's trace processor (pinned v58.2, hash-checked) is downloaded only after the user agreed
  (`mdh perf setup`); the new error code `NEEDS_CONSENT` says so to agents.
- MCP: `mdh_perf`.

### UI consistency checks
- `mdh visual check`: rules on the compact tree (touch targets, labels, overlap, controls under system bars,
  duplicate labels) and text contrast on pixels; structural (dp) and pixel baselines per device profile, approved
  with `mdh visual approve`; the same screen at font scale 1.3, in dark mode and right to left (`--configs`).
- Flows run them at every checkpoint, configured by their `visual:` section. MCP: `mdh_visual`.

### Devices
- `mdh devices` lists startable emulators; `mdh devices use` sets the project's default device;
  `mdh emulator start|stop`. With nothing online, mdh asks before starting an emulator (agents get the options in
  the error and ask their user).

## 0.1.0

The first release: precise verification for coding agents on Android.

### Control
- Compact screens (~150 tokens) with session-stable refs, actions (`tap`, `long-press`, `type` with any Unicode,
  `swipe`, `scroll --until`, `key`, `wait`) that wait for the UI to settle and report only what changed, obscured
  elements and system overlays.
- Crashes, native crashes and ANRs with every result, de-noised stack traces and the steps that led there.
- An on-device helper reads the UI in ~10 ms; uiautomator remains the fallback.
- `mdh run`: Gradle probing, builds with `file:line` diagnostics, install-if-changed (the right ABI split),
  restart and the first screen.
- State: animations off and back, runtime permissions, data reset, deep links (`mdh state`, `mdh open`).

### Verification
- `mdh verify`: checks on the screen and the logs (`visible`, `not visible`, `enabled`, `checked`, `text`,
  `screen`, `log`, always `no crash`) in one verdict with what was observed and evidence on disk.
- Flows: the session's recording saved as YAML (`mdh flow save`), replayed from a clean start with setup
  (`mdh flow run`), JUnit reports.
- `mdh impact`: the screens a code change reaches, how to reach them, broken call sites and what to verify,
  from tree-sitter analysis of Kotlin, Java and Android XML in ~150 ms, no device needed; `mdh flow run --changed`
  replays the flows the change needs.

### Integration
- MCP server (`mdh mcp`) with the same engine: `mdh_run`, `mdh_observe`, `mdh_act`, `mdh_wait`, `mdh_logs`,
  `mdh_app`, `mdh_verify`, `mdh_flow`, `mdh_impact`, `mdh_status`.
- Claude Code plugin with `verify` and `debug-crash` skills and hooks; `mdh init` adds a "how to verify" section
  to `AGENTS.md` for other agents.
- Output: text for agents and people, the `mdh/v1` JSON envelope with stable error codes and exit codes.
