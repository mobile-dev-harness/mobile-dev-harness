# 01 · Functional Design

> Status: draft v0.2 · Scope: the whole project (first release implements native Android; other platforms
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

## 3. Feature modules

Every feature has an ID `F<module>.<n>` that the roadmap and issues refer to.

### F1 Environment and devices

| ID | Feature | Behavior |
|---|---|---|
| F1.1 | `doctor` | Checks SDK, adb, emulator/AVDs, JDK, devices; each item reports ok/warn/fail with a fix hint ✅ |
| F1.2 | `devices` | Lists devices and their states ✅ |
| F1.3 | Emulator management | `emulator list/start/stop`; headless boot; returns only after `sys.boot_completed=1` |
| F1.4 | Device selection | Priority: `--device` flag > config > the only online device > boot the configured AVD > error listing candidates |
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
| F3.1 | Disable animations | Sets the three global animation scales to 0; restores original values when the session ends |
| F3.2 | Permissions | Grants or revokes runtime permissions; sets `appops` |
| F3.3 | System appearance | Locale, dark mode, font scale (for localization and accessibility checks) |
| F3.4 | Reset | Three levels: `none` / `data` (clear app data) / `snapshot` (load an emulator snapshot) |
| F3.5 | Snapshots | Save/load emulator snapshots to restore "logged in with test data" in seconds |
| F3.6 | Navigation | Opens a deep link by route name or URI |
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
| F5.3 | **Unicode input** | Typing Chinese and other non-ASCII text. `adb shell input text` can't, so M1 ships a minimal input helper (clipboard + paste); the full helper replaces it in M5 (architecture §10) |
| F5.4 | Waiting | Wait for an element to appear/disappear or for the UI to settle; every wait has a timeout; fixed sleeps are not offered |
| F5.5 | **Auto-observe after actions** | Each action waits for the UI to settle and returns a diff observation — "act and look" in one call, fewer round trips |

### F6 Verification

| ID | Feature | Behavior |
|---|---|---|
| F6.1 | Assertions | `visible` / `not_visible` / element state (enabled, checked, text, …) / `screen` (current screen) / `no_crash` / log contains or not / screenshot match (optional) |
| F6.2 | **Verdict** | Structured result: overall status, expected vs. observed per assertion, per-step timings, evidence paths (screenshots, log excerpts, crash report) |
| F6.3 | Evidence | Failures always include a screenshot and relevant logs; passes keep the final screenshot for human review |
| F6.4 | Visual regression (later) | Perceptual diff against a baseline screenshot, with masks for dynamic regions (clock, ad slots) |

### F7 Flows: record and replay

| ID | Feature | Behavior |
|---|---|---|
| F7.1 | Automatic recording | Every action in a session is recorded as a **selector-based step** (never coordinates); secret input is recorded as an env reference |
| F7.2 | Save | `flow save <name>` turns the recording (or a slice of it) into YAML; agents can edit it and add assertions |
| F7.3 | Replay | `flow run` executes deterministically, waiting for elements instead of sleeping; supports `setup` (reset, permissions, route) |
| F7.4 | CI reports | JUnit XML and exit codes; failures produce the same evidence as F6.3 |
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
| F9.2 | Claude Code plugin | MCP config, a `verify` skill (the verification protocol), hooks (inject environment status at session start; remind about unverified changes before stopping) |
| F9.3 | Other agents | `mdh init` can add a "how to verify this project" section to `AGENTS.md` for Codex, Cursor and others |

### F10 Platform and framework extensions (after the first release)

| ID | Feature | Behavior |
|---|---|---|
| F10.1 | React Native / Expo | Reuse or start Metro; detect RedBox/LogBox errors; collect JS logs and JS stacks |
| F10.2 | Flutter | `flutter build apk`; collect Flutter logs; rely on the Flutter semantics tree |
| F10.3 | iOS | Drive the simulator with `simctl` plus an accessibility tool; build with `xcodebuild` and parse xcresult diagnostics |

## 4. Interfaces

### 4.1 CLI command tree

```
mdh doctor | devices
mdh emulator list | start [avd] | stop
mdh init                              # probe the project, write mdh.yaml
mdh run [--route R] [--reset data|snapshot] [--no-build]
mdh build | install | launch
mdh observe [--detail minimal|normal|full] [--screenshot [--annotate]]
mdh tap <ref|selector> | type <ref|selector> <text> | swipe ... | scroll ... | back | home | key <k>
mdh wait <selector> [--gone] [--timeout 10s]
mdh open <route|uri>
mdh state animations off|restore | grant <perm> | reset data | snapshot save|load <name> | locale <tag> | dark on|off
mdh logs [--since last|<time>] [--level W] [--crash]
mdh verify <assertions.yaml | -e '<inline assertion>'>
mdh flow save <name> | list | run <name...> [--junit out.xml]
mdh session show | reset
mdh mcp
```

Global flags: `--json`, `--device <serial>`, `--project <dir>`, `-v`.

### 4.2 MCP tools

Few, coarse tools: every tool definition costs agent context, so actions are distinguished by parameters rather
than split into many small tools.

| Tool | Purpose | CLI equivalent |
|---|---|---|
| `mdh_status` | Environment, devices, project, session overview | doctor / devices / session |
| `mdh_run` | Build → install → launch → first observation | run |
| `mdh_observe` | Observe (detail level, screenshot, annotation) | observe |
| `mdh_act` | Run one or more actions, return a diff observation | tap / type / swipe / … / wait |
| `mdh_navigate` | Open a route or deep link | open |
| `mdh_state` | Permissions, reset, snapshots, appearance, animations | state |
| `mdh_logs` | Incremental logs, crash reports | logs |
| `mdh_verify` | Run assertions or a flow, return a verdict | verify / flow run |
| `mdh_flow` | Save and list flows | flow save / list |

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
```

### 4.6 Flow files

```yaml
name: login-success
setup:
  reset: data
  route: login
steps:
  - type: { id: email_input, text: alice@example.com }
  - type: { id: password_input, text: ${secrets.TEST_PASSWORD} }
  - tap: { text: Sign in }
  - wait: { id: inbox_list, timeout: 10s }
assert:
  - screen: .HomeActivity
  - visible: { text: Inbox }
  - no_crash
```

### 4.7 Project directory layout

```
mdh.yaml                 # config (committed)
.mdh/
  flows/                 # flows (committed)
  baselines/             # visual regression baselines (committed)
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
