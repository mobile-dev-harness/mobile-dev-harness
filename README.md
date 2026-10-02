**English** | [简体中文](README.zh-CN.md)

# mobile-dev-harness

**Let coding agents run, check and debug your Android app — the way they already check web apps in a browser.**

When an agent changes a web app, it can open a browser, click around, read the console and see whether the change
works. When it changes a mobile app, it usually can't: it edits code and hopes. `mobile-dev-harness` (command:
`mdh`) gives agents eyes and hands on a real device or emulator, designed around how agents work:

- **Compact screens.** The current screen as a short tree of elements (~150 tokens instead of thousands for raw
  XML), each with a ref like `e12` that stays the same for the whole session.
- **Act, then see what changed.** Every action waits until the UI has settled and reports only the difference.
- **Crashes surface immediately.** New errors, crashes, native crashes and ANRs come with every result, with the
  app's own stack frames and the steps that led there.
- **Fast.** A warm on-device helper reads the UI in ~10 ms (uiautomator takes ~2 s) and types any Unicode text.
- **One engine, two interfaces.** A CLI for humans, scripts and shell-based agents, and an MCP server for agents
  such as Claude Code.

> **Status: early development.** Android only for now; it already drives real apps end to end. See
> [Status and roadmap](#status-and-roadmap).

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

And when the app crashes, the agent knows right away (exit code 5):

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

## Requirements

- macOS or Linux (Windows is untested)
- [Android SDK platform-tools](https://developer.android.com/tools/releases/platform-tools) (`adb`); `mdh` finds
  the SDK through `ANDROID_HOME`, `ANDROID_SDK_ROOT` or the default Android Studio location
- An emulator or a device with USB debugging, Android 8.0 (API 26) or newer
- [Rust](https://rustup.rs) 1.85 or newer, to install from source (prebuilt binaries are planned)

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
| `mdh_observe` | The current screen; optionally only what changed, or a screenshot |
| `mdh_act` | One or more actions (`tap`, `long_press`, `type`, `swipe`, `scroll`, `key`), each reporting what changed |
| `mdh_wait` | Wait until an element appears or disappears |
| `mdh_logs` | Recent log lines and crash reports |
| `mdh_app` | Launch, stop or install an app |
| `mdh_status` | Device and session status; switch device or reset |

Then ask your agent something like *"Open the sample app, log in with alice@example.com, and check that the
messages list shows up."* Results are the same compact text the CLI prints; a crash of the app is returned as an
error with the crash report.

Agents that only have a shell can use the CLI directly — every command takes `--json`.

## Commands

| Command | Description |
|---|---|
| `mdh doctor` | Check the toolchain and devices |
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
| 3 | Environment problem: no SDK, no device, helper unavailable |
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

Working today (Android): observing screens, acting on them, waiting, logs and crash reports, the CLI and the MCP
server. Planned, organized around five quality domains:

| | Domain | What's planned |
|---|---|---|
| ✅ | **Control** | Drive the app reliably (done) |
| ⏳ | **Build** | Build from source with readable compiler errors; one command to build, install and launch |
| ⏳ | **Verify** | Assertions, evidence-backed verdicts, recorded flows replayed as regression tests, Claude Code plugin |
| ⏳ | **UI consistency** | Baselines, design-mock comparison, layout checks across configurations, accessibility rules |
| ⏳ | **Performance** | Startup time, jank, memory and CPU against baselines |
| ⏳ | **Compatibility** | The same checks across Android versions, screen sizes, configurations and vendors |
| ⏳ | **More platforms** | React Native, Expo, Flutter, then iOS |

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
