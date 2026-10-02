# AGENTS.md

Guidance for coding agents working in this repository. Human contributors: see
[CONTRIBUTING.md](CONTRIBUTING.md). Architecture and roadmap: [docs/DESIGN.md](docs/DESIGN.md).

## Project

`mobile-dev-harness` (`mdh`) gives coding agents an edit → run → verify loop for mobile apps:
build/install, device state setup, compact UI/log observation, assertions and replayable flows,
exposed as a CLI and (from M1) an MCP server. Android first; iOS, React Native and Flutter later.

**Current state: M0 done.** Implemented: Android SDK discovery, `mdh doctor`, `mdh devices`.
Next: M1 (engine/session, output envelope, compact UI tree, diffs, screenshots, input,
logcat, crash detection, MCP server). Feature IDs like `F4.1` refer to docs/design/01-functional.md.

## Layout

```
crates/
  mdh-core/     shared types (Device, Platform, Error/Result) — no I/O
  mdh-driver/   Driver trait + android/ (SDK discovery, adb wrapper)
  mdh-cli/      package `mobile-dev-harness`, binary `mdh` (clap)
docs/DESIGN.md  design overview + roadmap; details in docs/design/, decisions in docs/adr/
```

Dependency direction is strictly downward: `cli/mcp → verify/build/state → ui/observe → driver → core`.
Never make a lower crate depend on a higher one. New crates planned in DESIGN.md (`mdh-ui`,
`mdh-observe`, `mdh-build`, `mdh-state`, `mdh-verify`, `mdh-mcp`) are added only when their milestone
starts — don't create empty placeholder crates.

## Commands

```sh
cargo build
cargo test                                   # unit tests; no device needed
cargo fmt --all
cargo clippy --all-targets -- -D warnings    # CI fails on any warning
cargo run -- doctor                          # check local Android toolchain
cargo run -- --json devices
```

Run `cargo fmt --all` and the clippy command before finishing any change; CI runs both on Linux and
macOS with `--locked`, so commit `Cargo.lock` changes.

## Conventions

- **Rust edition 2024, MSRV 1.85.** Don't use std APIs stabilized after 1.85 even if your local
  toolchain is newer.
- **Shared dependency versions live in the root `Cargo.toml`** under `[workspace.dependencies]`;
  crates reference them with `foo.workspace = true`. Every crate sets `[lints] workspace = true`.
- **Errors:** library crates return `mdh_core::Result` with typed `mdh_core::Error` variants;
  only `mdh-cli` uses `anyhow`. Error messages should tell the user what to do next
  (see `Error::ToolNotFound { hint }`).
- **External tools:** inside `mdh-driver`, run them through `crate::process::run` (async, `tokio::process`), which
  returns stdout or `Error::CommandFailed` with stderr. Don't shell out via `sh -c`.
- **Parsers are pure functions over `&str`**, separated from the code that runs the tool, and unit
  tested with real captured output as fixtures. Tool output is messy — e.g. `adb devices` may
  include `* daemon ...` lines, `emulator -list-avds` may include `INFO |` log lines,
  `java -version` prints to stderr. Capture such quirks in tests.
- **Resolve SDK tools from the SDK root before PATH** (`AndroidSdk::locate`). Most users don't have
  `emulator` on PATH. Account for `EXE_SUFFIX` on Windows.
- **Agent-facing output is a product surface.** Every command supports `--json`; JSON shapes are
  stable API (changing one is a breaking change). Human output should be compact and actionable.
  Optimize for token efficiency: no decorative output, no redundant fields.
- Match the existing style: short doc comments that explain *why*, not *what*; no comment noise.

## Testing

- Unit tests must not require a device, emulator, network or Android SDK.
- Device-dependent tests are gated behind `MDH_E2E=1` and must not run by default.
- When fixing a parser bug, first add the offending real output as a test case.
- For manual checks, a headless emulator can be started with
  `$ANDROID_HOME/emulator/emulator -avd <name> -no-window -no-audio -no-snapshot-save` and stopped
  with `adb emu kill`. (macOS has no `timeout` command; poll in a loop instead.)

## Commits and PRs

- Commit messages: imperative summary line (≤ 72 chars), blank line, body explaining why.
  No trailers.
- Keep changes scoped to one milestone item; update the milestone table in `docs/DESIGN.md` and
  `README.md` when a milestone item lands.
- Significant design decisions get an ADR in `docs/adr/` (never edit an accepted ADR; supersede it).
