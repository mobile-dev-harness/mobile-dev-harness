**English** | [简体中文](README.zh-CN.md)

# mobile-dev-harness

**Precise verification for coding agents on Android: more accurate and fewer tokens than an agent manages on its own.**

When an agent changes a web app, it can open a browser, click around, read the console and see whether the change
works. When it changes a mobile app, it usually can't: it edits code and hopes. `mobile-dev-harness` (command:
`mdh`) gives agents eyes and hands on a real device or emulator, designed around how agents work:

- **Compact screens.** The current screen as a short tree of elements (~150 tokens instead of thousands for raw
  XML), each with a ref like `e12` that stays the same for the whole session.
- **Act, then see what changed.** Every action waits until the UI has settled and reports only the difference.
- **Crashes surface immediately.** New errors, crashes, native crashes and ANRs come with every result, with the
  app's own stack frames and the steps that led there.
- **Fast.** A warm on-device helper reads the UI in ~10 ms (uiautomator takes ~2 s) and types any Unicode text.
- **Knows what a change reaches.** `mdh impact` reads the uncommitted change and the project's Kotlin, Java and
  resource files — no device, no build, ~150 ms on a 300-file app — and lists the screens it affects, how to reach
  each one, the call sites of changed signatures and what to verify.
- **Verdicts, not impressions.** `mdh verify` checks the app (`enabled id=sign_in`, `screen .MessagesActivity`,
  always `no crash`) and answers pass or fail with what it observed and a screenshot on disk; what the agent did
  can be saved as a flow and replayed from a clean start, with JUnit for CI.
- **Build errors an agent can act on.** `mdh run` builds with Gradle, installs only what changed and restarts the
  app; compiler, resource, manifest and dependency failures come back as `file:line` with the offending line.
- **One engine, two interfaces.** A CLI for humans, scripts and shell-based agents, and an MCP server for agents
  such as Claude Code.

> **Status: early development.** Android only for now; it already drives real apps end to end. See
> [Status and roadmap](#status-and-roadmap).

## Why agents get better at mobile with it

Models reason well; they perceive and time poorly. On mobile, an agent's mistakes are rarely in the code it writes
and mostly in what it believes about the running app: it calls a clean compile "done", reads a screenshot taken
mid-transition, misses the crash in logcat, or checks the button it changed but not the screen it broke. `mdh`
replaces *eyeballing a screen* with structured facts and deterministic checks:

- **More accurate**: fewer false passes ("it works" when it doesn't) and fewer false fails ("it's broken" when it
  isn't), because the agent knows every screen its change reaches and sees settled states, every side effect and
  every crash.
- **Fewer tokens**: a screen in ~150 tokens instead of a ~1,500-token screenshot or thousands of tokens of XML, and
  only the diff after each action — so verifying after every change is affordable.

## What it looks like

```
$ mdh observe
screen dev.mdh.sample/.LoginActivity  1344x2992
[e69] textbox "Email" empty #email
[e70] textbox "Password" empty #password
[e71] checkbox "Remember me" unchecked #remember
[e72] button "SIGN IN" disabled #sign_in

$ mdh type "alice@example.com" --into Email
$ mdh type "correct-horse" --into Password
type •••• into e70 textbox "Password" → ok (783 ms)
screen dev.mdh.sample/.LoginActivity  1344x2992  keyboard
~ [e70] textbox "Password": value empty → ••••
~ [e72] button "SIGN IN": disabled → enabled
```

A toggle shows its side effects, not just itself:

```
$ mdh tap "Airplane mode"
tap e20 switch "Airplane mode" → ok (923 ms)
screen com.android.settings/.SubSettings  1344x2992
~ [e18] item "Internet": detail "AndroidWifi" → "Airplane mode is on"
~ [e19] item "SIMs": enabled → disabled
~ [e20] switch "Airplane mode": off → on
```

Before verifying, the agent learns what its change reaches — here, a signature change breaks a caller on another
screen, and the login screen no longer goes to the messages list:

```
$ mdh impact
impact vs HEAD (b7197c3): 2 files changed · 2 declarations
changed
  ~ LoginActivity.onCreate  body  LoginActivity.kt:15
  ~ Checkout.pay            signature (cartId: String) → (cartId: String, retry: Boolean)  TroublesActivity.kt:57
before → after
  LoginActivity.onCreate: + SettingsActivity::class · - MessagesActivity::class · - finish()
affected screens
  LoginActivity     via LoginActivity.onCreate · reach: mdhsample://login | MainActivity ▸ "Log in" ▸ LoginActivity
  TroublesActivity  via Checkout.pay → TroublesActivity.onCreate · reach: MainActivity ▸ "Troubles" ▸ TroublesActivity
callers of changed signatures
  Checkout.pay: TroublesActivity.kt:21 in TroublesActivity.onCreate — 1 argument, needs 2
verify
  functional     LoginActivity, TroublesActivity
note: syntax only: reflection, dependency injection, generated code and routes built at run time are not followed
```

The harness judges, not the agent: checks come back with what was observed, and evidence stays on disk:

```
$ mdh verify 'screen .LoginActivity' 'enabled id=sign_in' 'not visible id=error'
verdict: FAIL · 3 of 4 checks passed · 3.7 s
  ✓ screen .LoginActivity
  ✗ enabled id=sign_in — disabled: [e72] button "SIGN IN" disabled #sign_in
  ✓ not visible id=error
  ✓ no crash
evidence: .mdh/runs/1790957616814-verify (screenshot.jpg, tree.txt, logs.txt)
```

What the agent did becomes a regression test, replayed from a clean start; a crash anywhere fails it:

```
$ mdh flow save login-success --check 'screen .MessagesActivity'
saved .mdh/flows/login-success.yaml (4 steps, 1 check); set MDH_PASSWORD before running it
$ MDH_PASSWORD=… mdh flow run login-success troubles-crash --junit report.xml
verdict login-success: PASS · 4 steps · 2 of 2 checks passed · 5.8 s
  ✓ screen .MessagesActivity
  ✓ no crash
evidence: .mdh/runs/1790958025069-flow-login-success (screenshot.jpg, tree.txt, logs.txt)

verdict troubles-crash: FAIL · stopped after 1 of 3 steps · 0 of 2 checks passed · 3.1 s
  ✗ step 2: tap "Crash (Java)" — the app crashed (report below)
  ✗ no crash — java.lang.IllegalStateException: Sample crash: could not pay for the cart
    !! CRASH dev.mdh.sample (pid 26651): java.lang.IllegalStateException: Sample crash: could not pay for the cart
         at dev.mdh.sample.Checkout.pay(TroublesActivity.kt:61)
         …
evidence: .mdh/runs/1790958184747-flow-troubles-crash (screenshot.jpg, tree.txt, logs.txt)

flows: 1 of 2 passed
```

And when the app crashes during an action, the agent knows right away (exit code 5):

```
$ mdh tap "Crash (Java)"
tap e114 button "CRASH (JAVA)" → ok (979 ms)
screen dev.mdh.sample/.MainActivity  1344x2992  overlay:android
!! CRASH dev.mdh.sample (pid 20272): java.lang.IllegalStateException: Sample crash: could not pay for the cart
     at dev.mdh.sample.Checkout.pay(TroublesActivity.kt:61)
     at dev.mdh.sample.TroublesActivity.onCreate$lambda$0(TroublesActivity.kt:21)
     … 12 more frames
   caused by: java.lang.IllegalArgumentException: cart id must not be blank
   after: key BACK → tap e7 button "TROUBLES" → tap e114 button "CRASH (JAVA)"
[e126] "mdh sample keeps stopping"
[e127] button "App info"
[e128] button "Close app"
```

## Architecture

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/architecture-dark.svg">
  <img alt="Architecture: coding agents use mdh mcp and people, CI and scripts use the mdh CLI; both drive the same engine. Control (done) drives the app; the verification engine runs flows and pluggable checks — functional (done), UI consistency, performance (planned) — and the compatibility matrix repeats them across devices and configurations (planned); all on a shared foundation (observe, project, driver, core). On the Android device, the mdh helper keeps a UiAutomation connection warm and talks to the driver over adb forward in about 10 ms; crashes, ANRs and errors flow from logcat into observe." src="docs/assets/architecture-light.svg">
</picture>

- **Two entry points, one engine.** Agents connect over MCP, people, CI and scripts use the CLI; both get the same
  compact text.
- **Control, then verification.** Control (done) drives the app. On top of it, a verification engine runs flows
  and pluggable check kinds — functional, UI consistency, performance — into one verdict, and the compatibility
  matrix repeats all of it across devices and configurations (see [Status and roadmap](#status-and-roadmap)).
- **A warm helper on the device.** `dev.mdh.helper` keeps an accessibility connection open, so reading the screen
  and injecting input take milliseconds; logcat feeds crash and error reports back into every result.

The diagram is generated from code ([`scripts/diagram`](scripts/diagram)) in the style of Excalidraw.

## Why Rust: verification is the bottleneck

With today's models, writing the code is rarely what slows an AI coding task down — checking it is. An agent edits,
builds, runs, looks, fixes and repeats; one task can take dozens of verification rounds, and every round is spent
waiting on the harness and on the model reading what it returned. So the part that verifies has to be fast in
three ways:

1. **Little time per call.** `mdh` starts in about 5 ms — an empty Python or Node process alone takes 30–40 ms on the
   same machine, before loading anything — and does its device I/O concurrently: one observation reads the UI
   tree, the foreground activity and new logs in parallel. When an agent drives the CLI, every step is a new
   process, so startup is paid on every call.
2. **Little time for the model to read.** About 150 tokens per screen and only diffs after actions; the model's
   reading time and cost grow with every token.
3. **Nothing in the way.** One static binary (~6.5 MB) with the device helper embedded: no runtime, no dependencies
   to install, the same behavior in CI as on a laptop.

| Measured on an API 36 emulator | Time |
|---|---|
| Read the UI with `uiautomator dump` | ~2,000 ms |
| Read the UI with the `mdh` helper | ~10 ms |
| Start the `mdh` process | ~5 ms |
| `mdh observe`, end to end (tree, activity, logs) | ~100 ms |
| An action until the UI has settled | 0.8–1.4 s, mostly the app's own animations |

To be fair to other languages: the largest wins come from the design — a warm helper instead of `uiautomator`,
diffs instead of full screens. Rust is what keeps the harness itself from adding anything on top, and keeps it that
way as the planned performance, compatibility and visual checks do far more work per call than today.

## Requirements

- macOS or Linux (Windows is untested)
- [Android SDK platform-tools](https://developer.android.com/tools/releases/platform-tools) (`adb`); `mdh` finds
  the SDK through `ANDROID_HOME`, `ANDROID_SDK_ROOT` or the default Android Studio location
- An emulator or a device with USB debugging, Android 8.0 (API 26) or newer
- [Rust](https://rustup.rs) 1.88 or newer, to install from source (prebuilt binaries are planned)

## Install

```sh
cargo install --git https://github.com/qkmaosjtu/mobile-dev-harness mobile-dev-harness
mdh doctor
```

`mdh doctor` checks the SDK, adb, emulators, the JDK and connected devices, and says how to fix what's missing:

```
✓ android-sdk  /Users/you/Library/Android/sdk
✓ adb          Android Debug Bridge version 1.0.41 (/Users/you/Library/Android/sdk/platform-tools/adb)
✓ emulator     AVDs: Pixel_9_Pro_XL
✓ java         openjdk version "17.0.16" 2025-07-15
✓ devices      1 connected
```

On first use, `mdh` installs a small helper app on the device (`dev.mdh.helper`, ~11 KB, embedded in the binary).

## Quick start

In your app's project directory:

```sh
mdh run                              # build, install if changed, restart, show the first screen
```

```
build :assembleDebug → failed (1.2 s), 2 errors
e: src/main/kotlin/dev/mdh/sample/LoginActivity.kt:29:9 Unresolved reference 'emial'.
      29 |         emial.doAfterTextChanged { update() }
e: src/main/kotlin/dev/mdh/sample/LoginActivity.kt:33:30 Assignment type mismatch: actual type is 'String', but 'Boolean' was expected.
      33 |             signIn.isEnabled = "no"
full log: .mdh/runs/1790948182148-build/build.log
```

Any installed app works too:

```sh
mdh launch com.android.settings      # start an app; its logs and crashes are watched from now on
mdh observe                          # see the screen
mdh tap "Network & internet"         # by label…
mdh tap e20                          # …or by ref
mdh scroll down --until "System"
mdh key back
mdh logs                             # recent warnings, errors and crash reports
```

To try every feature, use the [sample app](#sample-app).

## Use it with an agent (MCP)

`mdh mcp` serves the same engine over [MCP](https://modelcontextprotocol.io) on stdio.

**Claude Code:**

```sh
claude mcp add mdh -- mdh mcp
```

**Other MCP clients:**

```json
{ "mcpServers": { "mdh": { "command": "mdh", "args": ["mcp"] } } }
```

| Tool | What it does |
|---|---|
| `mdh_run` | Build, install if changed, restart and show the first screen; build errors as `file:line` diagnostics |
| `mdh_observe` | The current screen; optionally only what changed, or a screenshot |
| `mdh_act` | One or more actions (`tap`, `long_press`, `type`, `swipe`, `scroll`, `key`), each reporting what changed |
| `mdh_wait` | Wait until an element appears or disappears |
| `mdh_logs` | Recent log lines and crash reports |
| `mdh_app` | Launch, stop or install an app |
| `mdh_verify` | Check the app now (`visible`, `enabled`, `text`, `screen`, `no crash`, …) or replay saved flows; a verdict with what was observed and evidence on disk |
| `mdh_flow` | Save what you did as a flow (with checks), list flows, show one |
| `mdh_impact` | What the uncommitted change (or the change since a ref) reaches: affected screens and how to reach them, broken call sites, what to verify |
| `mdh_status` | Device and session status; switch device or reset |

Then ask your agent something like *"Open the sample app, log in with alice@example.com, and check that the
messages list shows up."* Results are the same compact text the CLI prints; a crash of the app is returned as an
error with the crash report.

Agents that only have a shell can use the CLI directly — every command takes `--json`.

## Commands

| Command | Description |
|---|---|
| `mdh doctor` | Check the toolchain and devices |
| `mdh run [--project DIR] [--module M] [--variant V] [--no-build] [-g] [--reinstall]` | Build with Gradle, install if changed (the right ABI split), restart the app, show its first screen; `--reinstall` replaces an app signed with another key |
| `mdh devices` | List connected devices and emulators |
| `mdh observe [--diff]` | The current screen; `--diff` shows only what changed since you last looked |
| `mdh screenshot [-o FILE] [--max-edge 1024]` | Save a downscaled JPEG |
| `mdh tap TARGET` | Tap an element |
| `mdh long-press TARGET [--duration-ms 800]` | Long-press an element |
| `mdh type TEXT [--into TARGET] [--append] [--enter]` | Set the text of the focused field (any language) |
| `mdh swipe X1 Y1 X2 Y2 [--duration-ms 300]` | Swipe between coordinates |
| `mdh scroll up\|down\|left\|right [--in TARGET] [--until TARGET]` | Scroll, optionally until something is on screen |
| `mdh key NAME` | Press a key: `back`, `home`, `enter`, … |
| `mdh wait TARGET [--gone] [--timeout 10]` | Wait for an element to appear (or disappear) |
| `mdh logs [--level warn] [--lines 50]` | Recent logs and crash reports of the app |
| `mdh verify CHECK... [--timeout 3]` | Check the app as it is now and print a verdict (exit 1 on failure); checks: `visible T`, `not visible T`, `enabled\|disabled\|checked\|unchecked\|focused T`, `text T == V`, `text T ~= V`, `screen ACTIVITY`, `no crash`, `log ~= TEXT`, `no log ~= TEXT` |
| `mdh flow save NAME [--last N] [--check CHECK]... [--force]` | Save the session's recorded steps as `.mdh/flows/NAME.yaml` |
| `mdh flow run NAME... [--junit FILE] [--step-timeout 10]` · `mdh flow list` · `mdh flow show NAME` | Replay flows from a clean start, one verdict each |
| `mdh impact [--project DIR] [--base REF]` | What the change since `REF` (default `HEAD`: the uncommitted change) reaches and what to verify; no device needed |
| `mdh launch APP` · `mdh stop PACKAGE` · `mdh install APK [-g]` | App lifecycle |
| `mdh session show` · `mdh session reset` | Inspect or reset the session |
| `mdh mcp` | Serve the tools over MCP |

Global options: `--device SERIAL` (when several devices are connected) and `--json`.

### Targets

Wherever a command takes a `TARGET`, you can write:

| Form | Example | Notes |
|---|---|---|
| Ref | `e12` | From the last observation; stable for the whole session |
| Label | `"Sign in"` | Exact, then case-insensitive, then contains |
| Selector | `id=login`, `text=Sign in`, `text~=sign`, `role=switch` | Combine with `;`: `role=switch;text=Wi-Fi` |
| Coordinates | `540,1200` | Last resort |

When a target isn't found, the error lists the closest matches; when a ref is no longer on screen, it says what
the element was and where you saw it.

### Output and exit codes

Text output is meant to be read by agents and people. With `--json`, every command prints the same envelope:

```json
{ "schema": "mdh/v1", "ok": false, "data": null,
  "error": { "code": "ELEMENT_NOT_FOUND", "message": "…", "hint": "closest matches: …" },
  "warnings": [], "timing_ms": { "total": 75 } }
```

| Exit code | Meaning |
|---|---|
| 0 | Success |
| 1 | The app didn't match expectations: element not found, ambiguous, obscured, or a wait timed out |
| 2 | Invalid arguments or target |
| 3 | Environment problem: no SDK, no device, no Gradle project, helper unavailable |
| 4 | The build failed |
| 5 | The app crashed or stopped responding (ANR) |
| 10 | Internal error |

## How it works

- **Session.** Consecutive commands share a session (stored in `.mdh/session.json` in the current directory; the
  MCP server keeps one per connection). That's what keeps refs stable and lets results show only what changed.
  `mdh session reset` starts over.
- **On-device helper.** `dev.mdh.helper` keeps an accessibility connection open, so reading the screen takes
  milliseconds instead of seconds, and it can type Unicode text, which `adb shell input` can't.
- **Waiting.** After each action `mdh` waits until the UI stops changing, including spinners and loading states,
  and notices when the app stops responding.
- **Logs.** `mdh` reads logcat incrementally and attributes lines to your app by process, so you only hear about
  new problems.

Things to know:

- **One accessibility client per device.** While `mdh` is in use, other tools that use UiAutomation on the same
  device (uiautomator, Appium, Maestro, mobile-mcp) can't, and vice versa. Run `mdh session reset` to stop the
  helper.
- **No telemetry.** Nothing leaves your machine.

## Sample app

[`examples/android-sample`](examples/android-sample) is a small app made to exercise every feature: View and Compose
screens, a login form, a 100-row list, linked switches, a WebView, deep links, a runtime permission, a deliberate
edge-to-edge layout bug, and buttons that crash, crash natively, freeze (ANR), load slowly and log errors.

```sh
cd examples/android-sample && ./gradlew assembleDebug
mdh install build/outputs/apk/debug/mdh-sample-debug.apk
mdh launch dev.mdh.sample
```

Test account: `alice@example.com` / `correct-horse`.

## Status and roadmap

Working today (Android): building and running from source, observing screens, acting on them, waiting, logs and
crash reports, change impact analysis, verdicts with evidence, flows saved and replayed (JUnit), the CLI and the
MCP server. Planned: verification as an engine with pluggable check kinds, and a
matrix that repeats it across devices:

| | Layer | What's planned |
|---|---|---|
| ✅ | **Control** | Drive the app reliably (done) |
| ✅ | **Build** | Build from source with readable compiler errors; one command to build, install and launch (done) |
| ✅ | **Change impact** | Which screens a code change reaches and what to verify there, from static analysis (done) |
| ⏳ | **Verification engine** | One evidence-backed verdict per run, flows recorded and replayed as regression tests, JUnit reports (done); baselines and the Claude Code plugin next — with pluggable check kinds: |
| ✅ | ↳ **Functional checks** | Assertions on screens and logs: does it do what it should? (done) |
| ⏳ | ↳ **UI consistency checks** | Baselines, design-mock comparison, layout checks across configurations, accessibility rules |
| ⏳ | ↳ **Performance checks** | Startup time, jank, memory and CPU against baselines |
| ⏳ | **Compatibility matrix** | All of the above across Android versions, screen sizes, configurations and vendors |
| ⏳ | **More platforms** | React Native, Expo, Flutter, then iOS |
| ⏳ | **Benchmark** | Seeded-bug tasks measuring false passes, false fails, success and tokens: agent alone vs. adb and screenshots vs. mobile-mcp vs. mdh |

Details: [design overview](docs/DESIGN.md), [functional design](docs/design/01-functional.md),
[architecture](docs/design/02-architecture.md), [decision records](docs/adr/).

## Contributing

Issues and discussion are very welcome, especially reports of screens that `mdh` describes poorly — the output of
`mdh observe` plus a screenshot helps a lot. See [CONTRIBUTING.md](CONTRIBUTING.md); coding agents should also read
[AGENTS.md](AGENTS.md).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this project by
you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or
conditions.
