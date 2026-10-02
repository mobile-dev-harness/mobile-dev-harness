# mobile-dev-harness

**Give coding agents a real edit → run → verify loop for mobile apps.**

Web developers' agents can open a browser, click around, read the console and prove a change works.
Mobile agents mostly can't. `mobile-dev-harness` (`mdh`) closes that gap: it builds and installs your
app, prepares device state, observes UI and logs in a token-efficient form, and returns structured,
evidence-backed verdicts — exposed as a CLI and an MCP server, with first-class Claude Code integration.

> **Status: early development (M0).** Android first; iOS, React Native and Flutter are planned.
> See [docs/DESIGN.md](docs/DESIGN.md) for architecture and roadmap.

## Why not just a device-control MCP?

Tapping and screenshotting is only one layer. Agents also need to:

1. **Build & install** without rediscovering the Gradle setup every session
2. **Follow a verification protocol** instead of declaring victory after one screenshot
3. **Prepare state** — permissions, deep links, test data, disabled animations
4. **Read logs & crashes** filtered to the app, with stack traces attached
5. **Replay** verified paths as regression flows
6. **Stay cheap & fast** — compact UI trees first, screenshots only when needed
7. **Handle framework differences** — native, React Native, Flutter, Expo

## Quick start

```sh
cargo install --path crates/mdh-cli   # crates.io release coming later

mdh doctor    # check Android SDK, adb, emulator/AVDs, JDK
mdh devices   # list connected devices and emulators
mdh --json devices
```

## Roadmap

| Milestone | Scope |
|---|---|
| M0 | Workspace, CI, `doctor`, `devices` ✅ |
| M1 | Install/launch, compact UI tree, screenshots, input, logcat, crash detection; MCP server |
| M2 | Gradle detection, incremental build, compiler-error summaries, `mdh run` |
| M3 | `mdh init` config, permissions, deep links, animations, data reset |
| M4 | Assertions, flow record/replay, verdicts; Claude Code plugin |
| M5 | Token/latency optimizations, on-device helper, public benchmark |

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT)
at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this
project by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
