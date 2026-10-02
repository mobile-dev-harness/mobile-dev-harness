# mobile-dev-harness

**A quality harness for coding agents on mobile.**

Agents working on web apps can open a browser, click around, read the console and prove a change works. Agents
working on mobile apps mostly can't. `mobile-dev-harness` (`mdh`) gives them five capabilities, exposed as a CLI and
an MCP server with first-class Claude Code integration:

| Domain | What the agent gets |
|---|---|
| **Control** | Drive the app: compact UI trees with stable refs, diffs after actions, fast input (incl. Unicode), app lifecycle |
| **Verify** | Evidence-backed verdicts, assertions, flows recorded and replayed as regression tests |
| **Performance** | Startup, jank, memory and CPU measured against baselines, not single noisy runs |
| **Compatibility** | The same checks across Android versions, form factors, configurations and vendors |
| **UI consistency** | Baseline and design-mock comparison, cross-config layout checks, accessibility rules |

> **Status: early development (M1).** Android first; iOS, React Native and Flutter are planned.
> See [docs/DESIGN.md](docs/DESIGN.md) for architecture and roadmap.

## Why not just a device-control MCP?

Device-control tools such as mobile-mcp cover the first domain — tapping, typing, screenshots. That is necessary
but not sufficient: agents also need to know whether the change *works*, whether it made the app *slower*, whether
it *breaks on other devices*, and whether the UI still *looks right*. Even for control, `mdh` focuses on what agents
need: ~150-token screen descriptions instead of multi-thousand-token XML, refs that stay stable across actions,
diffs instead of full re-reads, and a warm on-device helper that returns UI trees in ~10 ms instead of ~2 s.

## Quick start

```sh
cargo install --path crates/mdh-cli   # crates.io release coming later

mdh doctor    # check Android SDK, adb, emulator/AVDs, JDK
mdh devices   # list connected devices and emulators
mdh launch com.android.settings
mdh observe                      # current screen as a compact UI tree with refs
mdh tap "Network & internet"     # by label; prints what changed after the UI settled
mdh tap e20                      # by ref, stable for the whole session
mdh type "深色模式" --into "Search Settings"
mdh scroll down --until "System"
mdh wait "role=switch;text=Wi-Fi" --timeout 5
mdh key back
mdh logs                         # recent warnings/errors and crash reports of the app
mdh screenshot -o shot.jpg
mdh --json observe --diff
```

## Use it from an agent (MCP)

`mdh mcp` serves the same session engine over MCP on stdio. In Claude Code:

```sh
claude mcp add mdh -- mdh mcp
```

Other clients:

```json
{ "mcpServers": { "mdh": { "command": "mdh", "args": ["mcp"] } } }
```

Tools: `mdh_status`, `mdh_observe`, `mdh_act`, `mdh_wait`, `mdh_logs`, `mdh_app`. Results are the same compact text
the CLI prints; crashes of the app are reported as errors with the crash report and the steps that led to it.

## Roadmap

| Milestone | Scope |
|---|---|
| M0 | Workspace, CI, `doctor`, `devices` ✅ |
| M1 | **Control**: on-device helper ✅, observe/screenshot/input/app lifecycle ✅, session engine ✅, logs and crashes ✅, MCP server ✅; sample app and benchmark vs. mobile-mcp |
| M2 | Gradle detection, builds with structured compiler diagnostics, `mdh run` |
| M3 | `mdh init` / `mdh.yaml`, permissions, deep links, animations, resets, snapshots |
| M4 | **Verify**: assertions, verdicts with evidence, flow record/replay, Claude Code plugin → **0.1.0** |
| M5 | **UI consistency** v1: baselines, cross-config layout checks, accessibility rules |
| M6 | **Performance** v1: startup, frames, memory, CPU, baselines → 0.2.0 |
| M7 | **Compatibility** v1: device and configuration matrices on emulators and physical devices |
| M8 | React Native / Expo, Flutter, design-mock comparison, cloud and vendor devices, Maestro import |
| M9 | iOS |

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT)
at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this
project by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
