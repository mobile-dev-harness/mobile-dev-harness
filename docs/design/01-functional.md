# 01 · Functional Design

> Status: draft v0.3 · Scope: the whole project (first release implements native Android; other platforms
> and frameworks are reserved at the interface level)

## 1. Users and scenarios

### 1.1 Three kinds of users

| User | How they use it | What matters most |
|---|---|---|
| **Coding agent** (primary) | Calls MCP tools or the CLI | Compact, structured, actionable output; few round trips; failures say what to do next |
| **Developer** | Onboards with `mdh init`, edits `mdh.yaml`, reviews evidence | Zero-config start; trustworthy, reviewable evidence (screenshots, logs) |
| **CI** | Runs regressions with `mdh flow run` | Determinism, low flakiness, standard report formats |

### 1.2 Core scenarios

| # | Scenario | Expected loop |
|---|---|---|
| S1 | **Verify a UI change** | Agent edits a layout → `run` (build/install/launch) → navigate to the screen → `observe` → `verify` assertions → verdict with evidence |
| S2 | **Reproduce and fix a crash** | Navigate per the issue → trigger the crash → stack trace and preceding actions captured automatically → fix → replay the same path to confirm |
| S3 | **End-to-end for a new feature** | Multi-step interaction (log in, fill a form, submit) → verify the result screen → save as a flow |
| S4 | **Regression** | Saved flows replay locally or in CI with a JUnit report |
| S5 | **First-time setup** | `mdh init` detects the project and writes config; `mdh doctor` points out environment problems |
| S6 | **Catch a performance regression** | After a change, `perf` runs the startup and a scrolling flow N times → compares with the stored baseline → reports "cold start +180 ms (p50), janky frames 2% → 9%" with the traces that explain it |
| S7 | **Check compatibility** | `compat` runs the saved flows across a matrix (API 26/30/36 × phone/tablet/foldable × default/large font/RTL) → per-cell verdicts, failures deduplicated |
| S8 | **Check UI consistency** | `visual` compares screens with their baselines and design mocks and runs layout and accessibility rules → "button overlaps text at font scale 1.3", "icon button without label", "spacing 12 dp vs. 16 dp in design" |

## 2. Core concepts

| Concept | Definition |
|---|---|
| **Project** | The user's mobile project (first release: a Gradle project) |
| **Target app** | The app under verification, identified by `applicationId`; the harness only touches it by default |
| **Device** | An emulator or physical device, identified by its adb serial |
| **Session** | A continuous working context: the selected device, ref table, previous UI tree, log cursor, action recording |
| **Observation** | A description of the screen and app at a moment: current screen, compact UI tree (or a diff against the previous one), new log summary, optional screenshot |
| **Ref** | A short handle for an element in an observation (`e12`), stable for the same element within a session |
| **Selector** | A persistable way to locate an element (`id` / `text` / `desc` / `role`, combinable), used by flows; never coordinates |
| **Action** | One interaction: tap, type, swipe, scroll, back, key, wait, … |
| **Assertion** | A decidable condition over an observation or the logs |
| **Flow** | A named, replayable sequence of actions and assertions stored as YAML |
| **Verdict** | A verification result — `pass` / `fail` / `error` — with the expected vs. observed value of every assertion and evidence |
| **Run** | The artifact directory of one `run` / `verify` / `flow run` (screenshots, logs, verdict) |
| **Route** | A named deep link from config, e.g. `settings → example://settings` |
| **Impact** | What a source change reaches: changed declarations, their callers and users up to the screens that show them, and what to verify there |

## 3. Feature modules

Every feature has an ID `F<module>.<n>` that the roadmap and issues refer to. Modules map to the layers of ADR-0009:

| Layer | Modules |
|---|---|
| Control | F1 Environment and devices · F3 State setup · F5 Interaction |
| Verification engine | F6 Verification (verdicts, evidence, functional checks) · F7 Flows · F14 Change impact (what a change needs verified) |
| ↳ Check kinds | F6.1 Functional · F13 UI consistency · F11 Performance |
| Compatibility matrix | F12 Compatibility |
| Foundation | F2 Build (`mdh-project`) · F4 Observation and F8 Cost and speed (`mdh-observe`) · F10 Platforms and frameworks |
| Entry points | F9 Agent integration |

### F1 Environment and devices

| ID | Feature | Behavior |
|---|---|---|
| F1.1 | `doctor` | Checks SDK, adb, emulator/AVDs, JDK, devices; each item reports ok/warn/fail with a fix hint ✅ |
| F1.2 | `devices` | Lists devices (AVD, API level, state) and the emulators that can be started, marking the project's default ✅ |
| F1.3 | Emulator management ✅ | `emulator start [AVD] [--headless]` / `stop [device]`; returns only after `sys.boot_completed=1`, telling the new emulator apart from those already connected; the emulator outlives mdh; a failed start reports the emulator's own reason |
| F1.4 | Device selection ✅ | `--device` (serial or AVD name; a named AVD that isn't running is started) > the project's default (`.mdh/device.json`, local: an emulator by AVD, a phone by serial) > the only online device > the only emulator when phones are online too (functional checks prefer an emulator) > ask. Asking: at a terminal, a numbered choice that becomes the default; with nothing online, "start <AVD>? [Y/n]" (never started unasked). Without a terminal (agents, CI, MCP) the error lists the options and the agent asks its user |
| F1.5 | Physical-device guard | Destructive operations on physical devices (clearing data, changing global settings) require `--allow-device-changes` |

### F2 Build, install, launch

| ID | Feature | Behavior |
|---|---|---|
| F2.1 | Project probing | Finds the application module, variant, `applicationId`, APK output path and launch activity via a Gradle init script (architecture §8); cached by a hash of the build files |
| F2.2 | Build | `assemble<Variant>`; keeps the Gradle daemon alive; long builds report progress over MCP |
| F2.3 | **Build diagnostics** | Parses Kotlin/Java compiler errors, resource errors, dependency resolution failures and Gradle configuration errors into structured diagnostics (file, line, column, message); returns the first N, full log saved to the run directory |
| F2.4 | Install | Skipped when the APK hash is unchanged; `-r -t`; optional `-g` to grant runtime permissions |
| F2.5 | Launch | Cold or warm start; `am start -W` records startup time; can launch straight into a route |
| F2.6 | `run` | Build → install → launch → wait until stable → return the first observation; any failing step returns that step's diagnostics |

### F3 State setup

| ID | Feature | Behavior |
|---|---|---|
| F3.1 | Disable animations ✅ | Sets the three global animation scales to 0 (`mdh state animations off`, `mdh_status`), and does so for every flow run; restores the original values on `animations on`, session reset, the end of a flow run and when the MCP connection closes |
| F3.2 | Permissions ✅ (grant, revoke) | Grants or revokes runtime permissions (`mdh state grant|revoke`, `mdh_app`, flow `setup.permissions`); `appops` later |
| F3.3 | System appearance | Locale, dark mode, font scale (for localization and accessibility checks) |
| F3.4 | Reset (`none`, `data` ✅) | Three levels: `none` / `data` (clear app data: `mdh state clear-data`, flow `setup.reset`) / `snapshot` (load an emulator snapshot) |
| F3.5 | Snapshots | Save/load emulator snapshots to restore "logged in with test data" in seconds |
| F3.6 | Navigation ✅ (URIs) | Opens a deep link (`mdh open`, `mdh_app` `open`, flow `setup.open` and `open` steps); named routes come with `mdh.yaml` |
| F3.7 | Test data | Pushes files via `run-as` for debuggable builds, or sends an agreed broadcast to the app (optional cooperation; the app is never required to change) |
| F3.8 | Secrets | Config only holds `${env:NAME}` references; values never hit disk, output or recorded flows |

### F4 Observation

| ID | Feature | Behavior |
|---|---|---|
| F4.1 | **Compact UI tree** | Keeps only interactive or informative nodes, drops meaningless layout levels, annotates roles and states, assigns stable refs. Target: < 800 tokens for a typical screen |
| F4.2 | **Diff observation** | After an action, returns only what changed (added, removed, changed elements) by default. Target: < 200 tokens |
| F4.3 | Opaque-region detection | Large unlabeled areas — WebView, Canvas, Compose without semantics — are flagged and the agent is told to take a screenshot |
| F4.4 | Screenshots | Downscaled (long edge 1024 by default), JPEG; optional **annotated mode** draws element boxes with ref labels so the image and the tree line up |
| F4.5 | Current screen | Foreground activity/window, system dialogs (permission, ANR), soft keyboard state |
| F4.6 | Logs | Filtered to the app process (follows the new pid after a restart); level/tag filters; incremental reads via a cursor ("since the last observation") |
| F4.7 | **Crashes and ANRs** | Detects Java/Kotlin crashes, native crashes and ANRs; produces a crash report: exception, de-noised stack (framework frames folded), the last few actions before the crash |
| F4.8 | Piggybacked signals | Every observation carries a summary of new error logs and a crash flag since the last one, so agents don't have to poll separately |

### F5 Interaction

| ID | Feature | Behavior |
|---|---|---|
| F5.1 | Targeting | By ref (session-scoped) or selector (persistent); a stale ref is re-resolved through its selector, otherwise the error lists the closest candidates |
| F5.2 | Actions | tap, long-press, type (with clear), swipe, scroll (including "scroll until an element appears"), back, home, key, hide keyboard |
| F5.3 | **Unicode input** | Typing Chinese and other non-ASCII text. `adb shell input text` can't; the on-device helper sets the focused field's text through accessibility (architecture §10) |
| F5.4 | Waiting | Wait for an element to appear/disappear or for the UI to settle; every wait has a timeout; fixed sleeps are not offered |
| F5.5 | **Auto-observe after actions** | Each action waits for the UI to settle and returns a diff observation — "act and look" in one call, fewer round trips |

### F6 Verification engine and functional checks (`mdh-verify`)

| ID | Feature | Behavior |
|---|---|---|
| F6.1 | Functional checks ✅ | `visible` / `not visible` / element state (`enabled`, `disabled`, `checked`, `unchecked`, `focused`) / `text` equals or contains / `screen` (current activity) / `no crash` (always checked, over the whole session or flow, including crashes already shown) / `log` and `no log`. Screen checks are re-read until they hold or a timeout (default 3 s) passes, so a result that is still loading isn't a false fail. Screenshot match comes with F13 |
| F6.2 | **Verdict** ✅ | Structured result: overall status (pass, fail, error), one line per check with what was observed when it failed, steps completed, duration, the run directory. `error` means a check couldn't be made (the screen unreadable, the device or a tool gone, a target the flow can't resolve), never that the app failed it |
| F6.3 | Evidence ✅ | Every verdict writes `screenshot.jpg`, `tree.txt`, `logs.txt` and `verdict.json` to `.mdh/runs/<time>-verify/` (or `-flow-<name>/`); crash reports go into the verdict itself |
| F6.4 | Check kinds ✅ (interface) | Functional checks are built in; UI consistency (F13) and performance (F11) checks implement the same check interface, run on the same flows and report into the same verdict, so one run can say "functional: pass; UI: 2 deviations; cold start +180 ms" |

### F7 Flows: record and replay (`mdh-verify`)

Flows belong to the engine because every check kind runs on them: performance measures a flow, UI consistency
inspects the screens it passes, the compatibility matrix replays it on every cell.


| ID | Feature | Behavior |
|---|---|---|
| F7.1 | Automatic recording ✅ | Every action in a session is recorded as a **selector-based step** (never refs); password input is recorded as `${env:MDH_<FIELD>}` |
| F7.2 | Save ✅ | `flow save <name> [--last N] [--check …]` turns the recording (or its last N steps) into YAML with checks to run at the end; agents edit the file to change it |
| F7.3 | Replay ✅ | `flow run` restarts the app (or opens `setup.open`, after `reset: data` and `permissions` if given) and runs the steps; each step waits for its target instead of sleeping; the first step that can't run stops the flow with what was on screen; missing `${env:…}` variables fail before the app is touched |
| F7.4 | CI reports ✅ | `--junit FILE` and exit codes; failures produce the same evidence as F6.3 |
| F7.5 | Maestro import (later) | Imports the common subset of Maestro flows to ease migration |

### F8 Cost and speed

| ID | Feature | Behavior |
|---|---|---|
| F8.1 | Detail levels | `minimal` (diffs and anomalies only) / `normal` (compact tree) / `full` (full tree); `normal` by default, `minimal` after actions |
| F8.2 | Long-list folding | Structurally identical siblings beyond a threshold collapse into "… 37 more items, scroll to see" |
| F8.3 | Screenshot policy | No screenshot by default; taken on assertion failure, opaque-region detection, or explicit request |
| F8.4 | Timing transparency | Every result carries per-phase timings (build, install, dump, screenshot), reused directly by the benchmark |

### F9 Agent integration

| ID | Feature | Behavior |
|---|---|---|
| F9.1 | MCP server | `mdh mcp` (stdio); tool set in §4.2 |
| F9.2 | Claude Code plugin ✅ | `integrations/claude-code`, installable from this repository's marketplace: the MCP server, `verify` and `debug-crash` skills, and hooks — at session start the devices online and the saved flows; before stopping, a reminder (once) when app files changed during the session have no passing verdict since |
| F9.3 | Other agents ✅ | `mdh init` sets up `.mdh/` (flows committed, state ignored) and keeps a "how to verify this project" section in `AGENTS.md` for Codex, Cursor and others |

### F10 Platform and framework extensions (after the first release)

| ID | Feature | Behavior |
|---|---|---|
| F10.1 | React Native / Expo | Reuse or start Metro; detect RedBox/LogBox errors; collect JS logs and JS stacks |
| F10.2 | Flutter | `flutter build apk`; collect Flutter logs; rely on the Flutter semantics tree |
| F10.3 | iOS | Drive the simulator with `simctl` plus an accessibility tool; build with `xcodebuild` and parse xcresult diagnostics |

### F11 Performance checks (`mdh-perf`)

A check kind of the verification engine (F6.4): its measurements and budget results are findings in a verdict.


| ID | Feature | Behavior |
|---|---|---|
| F11.1 | Startup ✅ | `perf startup [app] [--hot]`: cold start (after force-stop) and hot start (after HOME) over N runs plus a discarded first one (`am start -W`); median, p90 and spread. Warm starts and time to full display (`reportFullyDrawn`) later |
| F11.2 | Rendering ✅ | `perf flow <name>`: frame timing while a saved flow runs, N times (`dumpsys gfxinfo` reset at the start, read at the end): janky-frame share, p90 and p99 frame time; animations kept on |
| F11.3 | Memory ✅ | Total PSS (`dumpsys meminfo`) at the end of each run; growth with every repetition of the same flow is reported as a leak signal |
| F11.4 | CPU ✅ | CPU time of the app process over the run's wall time (`/proc/<pid>/stat`) |
| F11.5 | Budgets and baselines ✅ | Budgets in the flow's `perf:` section (`janky_pct: 5`, `cold_start_ms: 800`, …) judged on the median; baselines per device profile (AVD or model, API level, debug/release) in `.mdh/baselines/perf/`, recorded by the first measurement, promoted by `perf approve`; a regression must exceed both three times the noise and a per-metric minimum, never judged on a single run |
| F11.6 | Traces ✅ | On a regression (or `--trace`) one more run under Perfetto, summarized to the main-thread work that explains it (`bindApplication 592 ms → … → slowInit 451 ms`, `RV Prefetch ×88 806 ms → … → slowBind ×33 792 ms`), late frames and GC; the trace is kept for ui.perfetto.dev. The host-side trace processor (14 MB, pinned, hash-checked) is downloaded only after the user agreed (`perf setup`) |

> Emulators are not representative in absolute terms. Reports name the device, compare against a baseline on the
> same device, and show variance; absolute budgets are meant for physical devices.

### F12 Compatibility (`mdh-compat`)

Not a check kind, and not a matrix run for its own sake (ADR-0011): the change says what is at risk, and only
that is verified. Impact (F14) → risk analysis → verification plan → verdict per risk.


| ID | Feature | Behavior |
|---|---|---|
| F12.1 | Risk analysis ✅ | `compat risks`: from the change's facts and a knowledge base shipped as data, the compatibility risks in four dimensions — **OS version** (API-level branches and `@RequiresApi` → both sides of the boundary; APIs whose behavior changed in a version; `targetSdk` raised → that version's behavior changes; `minSdk` changed → the lowest version), **device type** (qualified resources such as `sw600dp`, window size classes, folding features, `configChanges`, orientation and resizability, saved state; cars and TVs), **vendor** (background work, notifications, exact alarms, overlays, system settings intents, WebView, autostart and battery restrictions of Xiaomi, OPPO, vivo, Huawei, Honor, Samsung ROMs) and **screen size** (changed layouts and composables on the screens they reach). Each risk has its reason, evidence (`file:line`, the matched name), the screens, a likelihood and how to verify it. No device, milliseconds; `mdh impact` shows the risks too |
| F12.2 | Verification plan ✅ | `compat plan`: the fewest cells covering the risks, cheapest first: configuration on the current device (screen size and density overrides for a small phone, a tablet, a foldable's inner screen; landscape; font scale; dark mode; locale), then other local AVDs by API level, then connected physical devices by vendor. States what each cell costs and what would need consent (starting an emulator, downloading a system image); risks that can't be covered here are listed as unverifiable with what's missing |
| F12.3 | Run ✅ (emulators; vendor devices tested against scripted devices only) | `compat run [--changed]`: builds once, installs on each cell's device, applies the configuration (restored afterwards), runs the flows that pass the risks' screens with their checks (functional, UI rules), or opens the screens by deep link; device-type risks add a state check across rotation (and folding on a foldable AVD). Starts at most two new emulators, only with consent |
| F12.4 | Report ✅ | One line per risk: verified (on which cells), failed (cell, check, evidence), unverified (why, what would verify it); failures deduplicated across cells; simulated cells (display overrides) say so |
| F12.5 | Explicit matrix (later) | `compat run --cells …` for release testing: the same execution over cells given by hand |
| F12.6 | Device providers (later) | Creating AVDs from system images on demand, cloud device farms for vendors and form factors not at hand, car (AAOS) and TV emulators |

### F13 UI consistency checks (`mdh-visual`)

A check kind of the verification engine (F6.4): deviations and rule violations are findings in a verdict.


| ID | Feature | Behavior |
|---|---|---|
| F13.1 | Baselines ✅ | Per flow checkpoint (each `assert` step and the end) or named screen, per device profile: the compact tree in dp and a half-resolution screenshot. Comparison is structural first (elements added, missing, moved or resized beyond 4 dp, text changed — cheap and explains *what* changed), then pixels: 16 px blocks, changed regions named by the element they fall in, with masks for dynamic regions (system bars, keyboard, focused inputs, `ignore` elements, `mask` rectangles) and a diff image as evidence. The first run records the baseline; a deviation fails the check and leaves a candidate that `mdh visual approve` makes the baseline |
| F13.2 | Design mocks (later) | Imports frames from Figma (rendered image and node data), maps them to screens, and reports measured deviations of matched elements (position, size, spacing, color, font size) plus a side-by-side diff |
| F13.3 | Cross-config layout ✅ (on one device; truncation later) | `configs: [font_scale, dark, rtl]` on the last screen of a flow (or `mdh visual check --configs`): font scale 1.3, dark theme, the app in Arabic (right to left); each is switched on, checked and restored. Reports elements missing compared with the default configuration and rule violations that only appear there. On a matrix cell (M7) the same checks run per cell. Truncation needs data accessibility doesn't expose |
| F13.4 | Rule checks ✅ | On the tree: touch targets ≥ 48 dp, controls without a label, overlapping controls, controls under the system bars, duplicate labels (a warning); on pixels: text contrast against WCAG AA (4.5:1, 3:1 for large text), from the most common color (background) and the most contrasting color covering a visible share (text). In flows they fail the verdict when the flow lists them in `visual.rules` and are warnings otherwise. Content pushed off the screen can't be found in the tree (accessibility clips bounds to the visible area) |
| F13.5 | Design tokens (later) | Colors and text styles on screen checked against the design system's tokens |

### F14 Change impact analysis (`mdh-impact`, ADR-0010)

Tells the agent what its change reaches before it verifies anything, so it checks every affected screen and not
just the one it was editing. Static and syntax-level: no device, no build, works on code that doesn't compile.

| ID | Feature | Behavior |
|---|---|---|
| F14.1 | Change detection | Working tree (staged, unstaged and untracked files) against `HEAD`, or against `--base <ref>`; limited to the project directory |
| F14.2 | Declaration diff | Kotlin and Java classes, objects, functions and properties; Android resources (layouts, values entries, drawables and other resource files); manifest entries. Each is added, removed, signature changed or body changed; comment- and formatting-only changes are dropped |
| F14.3 | Before → after | Calls, navigation edges (`Intent(…, X::class.java)`, class literals of screens) and resource references gained or lost by the changed code |
| F14.4 | Callers and users | Reverse references across the project, resolved by name with receiver types, imports, package and scope; each with a confidence (`exact`, `likely`, `ambiguous`) and a location. Users of removed declarations are flagged |
| F14.5 | Affected screens | Callers followed up to activities, fragments and composables hosted by an activity, with the path (`padForSystemBars ← LoginActivity.onCreate`); how to reach each screen: deep links from the manifest, or the taps from the launcher screen (button labels from layouts) |
| F14.6 | What to verify | Functional: the affected screens. UI: changed layouts, resources, composables or themes. Performance: changes in list adapters, lazy lists, drawing, `Application`/launcher startup. Compatibility: manifest changes, qualified resources (`values-zh`, `layout-land`), `SDK_INT` branches. Tests: unit and instrumented tests that reference changed code. Flows ✅: the saved flows whose recorded `screens` include an affected screen or its host activity (all of them when the build configuration changed); `mdh flow run --changed` replays them |
| F14.7 | Honest limits | Lists what syntax can't see (reflection, dependency injection, generated code, routes built at run time) and changed files it doesn't analyze (build scripts are reported as "build configuration changed: verify the whole app") |
| F14.8 | Budgets | Each section has a line budget and reports what it folded; `--json` returns everything |

## 4. Interfaces

### 4.1 CLI command tree

```
mdh doctor
mdh devices [use <serial|avd>]
mdh emulator start [AVD] [--headless] | stop [serial|avd]
mdh init [--project DIR] [--no-agents-md]   # .mdh/ and the AGENTS.md section; mdh.yaml with the config track
mdh run [--project DIR] [--module M] [--variant V] [--no-build] [-g]   # --route/--reset with M3
mdh build | install | launch
mdh observe [--diff] [--detail minimal|normal|full] [--screenshot [--annotate]]
mdh screenshot [-o file] [--max-edge 1024]
mdh tap <target> | long-press <target> | key <name> | swipe x1 y1 x2 y2
mdh type <text> [--into <target>] [--append] [--enter]
mdh scroll up|down|left|right [--in <target>] [--until <target>]
mdh wait <target> [--gone] [--timeout 10]
mdh launch <package|component> | stop <package> | install <apk> [-g]
mdh open <uri> [--package P]                # routes with mdh.yaml
mdh state animations on|off | grant <perm> [--package P] | revoke <perm> | clear-data [package]
                                      # later: snapshot save|load <name> | locale <tag> | dark on|off
mdh logs [--level warn] [--lines 50]
mdh impact [--base REF] [--project DIR]   # what the change reaches; no device needed
mdh verify CHECK... [--timeout 3]     # e.g. 'visible "Sign in"' 'enabled id=sign_in' 'screen .LoginActivity'
mdh flow save <name> [--last N] [--check CHECK]... [--force] | list | show <name>
mdh flow run <name...> | --changed [--base REF]  [--junit out.xml] [--step-timeout 10] [--timeout 3]
mdh perf startup [app] [--hot] [--runs 5] [--trace] | flow <name> [--runs N] [--trace] | approve [SCOPE]
mdh perf setup [--yes] | explain <trace> --app <pkg> [--startup]   # Perfetto's trace processor; asks before downloading
mdh compat risks [--base REF] | plan [--base REF] | run [--changed] [--yes]
mdh visual check [--baseline NAME] [--rules all|none|r1,r2] [--ignore TARGET]... [--configs font_scale,dark,rtl] | approve [NAME|FLOW]
mdh session show | reset
mdh mcp
```

Global flags: `--json`, `--device <serial>`, `--project <dir>`, `-v`.

A `<target>` is a ref (`e12`), coordinates (`100,200`), a selector (`id=…`, `text=…`, `text~=…` for contains,
`role=…`, `index=…`, combined with `;`, e.g. `role=switch;text=Wi-Fi`) or a bare label (exact, then
case-insensitive, then contains). Actions print what they did, wait for the UI to settle and report what changed
since the agent last looked; CLI invocations share a session through `.mdh/session.json`.

### 4.2 MCP tools

Few, coarse tools: every tool definition costs agent context, so actions are distinguished by parameters rather
than split into many small tools.
The default set (`mdh mcp`, `--tools core`) is what an agent uses every turn: driving the app, building, verifying,
flows, impact, in 8 tools whose definitions take about 5.9k characters (a test keeps them under 6.5k). The check kinds
(`mdh_visual`, `mdh_perf`, `mdh_compat`) come with `--tools all`; agents with a shell use the CLI instead, guided
by skills loaded only when needed.

| Tool | Purpose | CLI equivalent | Since |
|---|---|---|---|
| `mdh_status` | Device and session overview; switch device (remembered), start an emulator the user agreed to, reset the session, animations on/off | devices / emulator / session / state animations | M1 ✅ |
| `mdh_observe` | Observe (diff, screenshot), or the app's logs and crash reports | observe / screenshot / logs | M1 ✅ |
| `mdh_act` | Run one or more actions (waiting for a target is one), each returning what changed | tap / type / scroll / … / wait | M1 ✅ |
| `mdh_app` | Launch, stop, install, open a deep link, clear data, grant/revoke permissions | launch / stop / install / open / state | M1 ✅, M4 ✅ |
| `mdh_run` | Build → install → launch → first observation, with progress | run | M2 ✅ |
| `mdh_state` | Snapshots, appearance (locale, dark mode, font scale) | state | M5, M7 |
| `mdh_impact` | What the uncommitted change (or the change since a ref) reaches and what to verify | impact | M4 |
| `mdh_verify` | Run checks or replay flows (named, or those the uncommitted change needs), return a verdict | verify / flow run | M4 ✅ |
| `mdh_flow` | Save, list and show flows | flow save / list / show | M4 ✅ |
| `mdh_visual` | Rule checks, structural and pixel baselines, other configurations of the current screen; approve candidates | visual | M5 ✅ (`--tools all`) |
| `mdh_perf` | Measure startup or a flow against baselines and budgets, explain regressions with a Perfetto trace; approve; set up the trace processor (with the user's consent) | perf | M6 ✅ (`--tools all`) |
| `mdh_compat` | Compatibility risks of the change, the plan to verify them, and the run: a verdict per risk | compat | M7 ✅ (`--tools all`) |

Screenshots are returned as MCP image content; long builds report via progress notifications.

### 4.3 Output contract

All `--json` output and MCP results use one envelope (see ADR-0005):

```json
{
  "schema": "mdh/v1",
  "ok": false,
  "data": null,
  "error": {
    "code": "ELEMENT_NOT_FOUND",
    "message": "no element matches text=\"Sign in\"",
    "hint": "closest matches: [e4] button \"Sign In\", [e9] link \"Sign up\"",
    "details": {}
  },
  "warnings": [],
  "timing_ms": { "total": 812, "ui_dump": 640 }
}
```

Exit codes: `0` success · `1` verification failed · `2` usage error · `3` environment problem (no SDK, no device) ·
`4` build failed · `5` app crashed · `10` internal error.

### 4.4 Observation text format (what the agent sees)

```
screen com.example/.LoginActivity  1080x2400  keyboard:up
[e1] textbox "Email" value="alice@example.com"
[e2] textbox "Password" value=•••• focused
[e3] checkbox "Remember me" checked
[e4] button "Sign in" disabled
[e5] link "Forgot password?"
logs: 1 warning since last (W/Auth: token cache miss)
```

Diff observation after an action:

```
tap e4 → ok (420ms)
screen com.example/.HomeActivity
+ [e7] list "Inbox" scrollable (24 items, 6 visible)
+ [e8] tab "Settings"
- e1..e5
```

JSON mode carries the same information in structured form.

### 4.5 Config: `mdh.yaml`

```yaml
version: 1
android:
  project: .                 # Gradle root
  module: app                # auto-detected when there is a single application module
  variant: debug
  # applicationId / launchActivity come from probing and can be overridden
  device:
    avd: Pixel_9_Pro_XL      # or serial: emulator-5554
build:
  gradleArgs: ["--offline"]
  skipInstallIfUnchanged: true
state:
  animations: off
  permissions: [android.permission.POST_NOTIFICATIONS]
  locale: zh-CN
  reset: none                # none | data | snapshot
  snapshot: logged-in
routes:
  home: example://home
  settings: example://settings
secrets:
  TEST_PASSWORD: ${env:MDH_TEST_PASSWORD}
observe:
  detail: normal
  screenshot: { maxEdge: 1024, format: jpeg }
flows: .mdh/flows
perf:
  runs: 10
  budgets:
    coldStartP50Ms: 800
    jankyFramesPct: 5
compat:
  matrices:
    default:
      api: [26, 30, 36]
      form: [phone, tablet, foldable]
      config: [default, { fontScale: 1.3 }, { locale: ar }]
visual:
  masks: [{ id: clock }, { region: status_bar }]
  rules: [touch_target, label, contrast]
```

### 4.6 Flow files

Saved in `.mdh/flows/<name>.yaml`; `mdh flow save` writes them, people and agents edit them.

```yaml
name: login-success
app: dev.mdh.sample            # restarted before the steps (unless setup.launch: false)
setup:
  reset: data                  # none (default) | data: clear the app's data first
  permissions: [android.permission.POST_NOTIFICATIONS]
  open: mdhsample://login      # start from a deep link instead of the launcher activity
  animations: true             # keep system animations; by default they're off during the run, then restored
screens: [LoginActivity, MessagesActivity]   # recorded on save; picks the flows a change needs
steps:
  - tap: "Log in"                                   # targets as on the CLI: label, id=…, text~=…, role=…
  - type: { into: id=email, text: alice@example.com }
  - type: { into: { id: password }, text: "${env:MDH_PASSWORD}" }   # targets also as maps
  - tap: id=sign_in
  - wait: { target: role=progress, gone: true, timeout: 20 }        # or `wait: TARGET`
  - scroll: { direction: down, until: "Message 30" }                # or `scroll: down`
  - key: back
  - open: mdhsample://settings
  - assert:                                         # checks in the middle of a flow
      - visible "Messages"
visual:                                             # UI consistency at every checkpoint (assert steps, the end)
  rules: [touch_target, label]                      # fail on these (`all`, `none`); unlisted: all rules, as warnings
  baseline: true                                    # compare with .mdh/baselines/visual/<flow>/<checkpoint>/
  ignore: [id=clock]                                # left out of the comparison
  mask: [[0, 0, 360, 24]]                           # [left, top, width, height] in dp, left out of the pixels
  pixels: true                                      # with baseline, compare pixels too (default)
  configs: [font_scale, dark, rtl]                  # also check the last screen in these configurations
assert:                                             # checks after the last step; `no crash` is implied
  - screen .MessagesActivity
  - text: { target: id=title, equals: Messages }    # the structured form of `text id=title == "Messages"`
```

Checks use the inline syntax of `mdh verify` (`visible TARGET`, `not visible TARGET`, `enabled|disabled|checked|
unchecked|focused TARGET`, `text TARGET == VALUE`, `text TARGET ~= VALUE`, `screen ACTIVITY`, `no crash`,
`log ~= TEXT`, `no log ~= TEXT`) or the same as YAML maps (`- enabled: { id: sign_in }`).

### 4.7 Project directory layout

```
mdh.yaml                 # config (committed)
.mdh/
  flows/                 # flows (committed)
  baselines/             # visual and perf baselines (committed)
  cache/                 # project probe cache (ignored)
  session.json           # CLI session state (ignored)
  runs/<time>-<id>/      # screenshots, logs, verdicts (ignored; last N kept)
```

## 5. Non-functional requirements

| Category | Target (adb backend / with helper) |
|---|---|
| Observation latency | < 1.5 s / < 300 ms |
| Action + observation | < 2 s / < 600 ms |
| `run` with no changes | < 5 s overhead beyond the build |
| Tokens | Typical screen observation < 800; diff < 200 |
| Stability | Flow replay flake rate < 1% on the sample app |
| Host platforms | macOS, Linux, Windows |
| Privacy | No telemetry; secrets never written to disk; password fields always masked |
| Compatibility | Android API 26+ (covers 95%+ of active devices) |

## 6. Non-goals

- Not a replacement for unit-level UI test frameworks such as Espresso or Compose UI Test; it complements them
  (later it may trigger and summarize their results).
- No cloud device farm, GUI recorder or deep performance profiling in the first release.
- No general-purpose phone automation (driving other apps); by default only the target app and the system
  dialogs it triggers.
