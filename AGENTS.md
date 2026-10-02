# AGENTS.md

Guidance for coding agents working in this repository. Human contributors: see
[CONTRIBUTING.md](CONTRIBUTING.md). Architecture and roadmap: [docs/DESIGN.md](docs/DESIGN.md).

## Project

`mobile-dev-harness` (`mdh`) is a quality harness for coding agents on mobile, organized around five
domains on a shared foundation (ADR-0008): **control**, **verify**, **performance**,
**compatibility** and **UI consistency**. Exposed as a CLI and (from M1) an MCP server. Android
first; iOS, React Native and Flutter later.

**Current state: M1 (control) in progress.** Implemented: the session engine in `mdh-control`
(session-stable refs, ref/selector/label targeting, wait-for-stable, diffs after actions, recorded
steps; CLI sessions in `.mdh/session.json`), `observe [--diff]`, `tap`, `long-press`, `type`,
`scroll`, `swipe`, `key`, `wait`, `screenshot`, `launch`/`stop`/`install`, `session show|reset`, `logs`; log digests and crash reports on every
observation (crash → exit 5); the MCP server (`mdh mcp`, six tools); the on-device helper; the
ADR-0005 output envelope; `examples/android-sample`. M1 is complete except the benchmark against
mobile-mcp, which is in the backlog (docs/DESIGN.md). Next milestone: M2 (project/build).
Feature IDs like `F4.1` refer to docs/design/01-functional.md.

## Layout

```
crates/
  mdh-core/      foundation: shared types, errors and codes, output envelope, timings — no I/O
  mdh-driver/    foundation: Driver trait + android/ (SDK, adb, helper client, uiautomator, parsers)
  mdh-observe/   foundation: compact UI trees, stable refs, rendering, diffs, screenshots; logs next
  mdh-project/   foundation: build adapters (Gradle first)                 — empty until M2
  mdh-control/   domain: device selection, observation, input, app lifecycle; session engine next
  mdh-verify/    domain: assertions, verdicts, flows                        — empty until M4
  mdh-visual/    domain: UI consistency                                     — empty until M5
  mdh-perf/      domain: performance                                        — empty until M6
  mdh-compat/    domain: compatibility matrices                             — empty until M7
  mdh-mcp/       entry point: MCP server over stdio (rmcp), compiled into `mdh mcp`
  mdh-cli/       entry point: package `mobile-dev-harness`, binary `mdh` (parsing + rendering only)
android-helper/  on-device helper APK (Java, no dependencies); see docs/design/02-architecture.md §10
examples/android-sample/  test app exercising every feature (Kotlin, Views + Compose)
scripts/         build-helper.sh rebuilds the helper into crates/mdh-driver/assets/
fixtures/        real tool output used by tests (e.g. android/uiautomator/<screen>_api<level>.xml)
docs/DESIGN.md   design overview + roadmap; details in docs/design/, decisions in docs/adr/
```

Dependencies point strictly downward: entry points → verify / perf / compat / visual → control →
observe / project → driver → core. `mdh-compat` may depend on verify, perf and visual (it
orchestrates them). Never make a lower crate depend on a higher one. Domain crates exist with their
scope documented in `lib.rs`; keep them empty until their milestone starts. Logic belongs in library
crates — the CLI only parses arguments and renders results (`crates/mdh-cli/src/render.rs`).

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
- **Errors:** all crates return `mdh_core::Result` with typed `mdh_core::Error` variants. Every
  variant maps to a stable `ErrorCode` (exit code via `ErrorCode::exit_code`) and must produce a
  `hint` telling the caller what to do next. Adding a variant means adding its code and hint.
- **Output:** library functions return serializable data; the CLI implements `Human` for it in
  `render.rs` and prints through `output::finish`, which emits the ADR-0005 envelope with `--json`.
  Phase timings go through `mdh_core::output::Timings`. Never `println!` results directly.
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
- Compressor output is pinned with `insta` snapshots (`crates/mdh-observe/tests/snapshots/`). After an
  intended output change, regenerate with `INSTA_UPDATE=always cargo test -p mdh-observe` and review the
  snapshot diff — including the token counts in the header — before committing.
- Fixture names carry the API level. Before saving a new dump, confirm the screen actually changed
  (`am start` may only bring an existing task to the front; use `-S`).
- For manual checks, a headless emulator can be started with
  `$ANDROID_HOME/emulator/emulator -avd <name> -no-window -no-audio -no-snapshot-save` and stopped
  with `adb emu kill`. (macOS has no `timeout` command; poll in a loop instead.)

## On-device helper

- Java only, no dependencies, minSdk 26; keep the APK tiny (it is embedded in the binary).
- After changing `android-helper/`, bump the version code in all three places (Gradle `versionCode`,
  `Commands.VERSION_CODE`, `HELPER_VERSION_CODE`), run `scripts/build-helper.sh` and commit the APK.
- JSON field names in `Commands.node()` / `windows()` must match `mdh_core::ui::{RawNode, WindowInfo}`.
- Inject input asynchronously; synchronous injection blocks for up to seconds while apps animate.

## Session engine

- Session logic is tested against the scripted `FakeDriver` in `crates/mdh-control/tests/session.rs`;
  add a scenario there for every behavior change (no device needed).
- Recorded steps must stay replayable: never record refs or coordinates for element targets, only
  selectors (`selector_for`). Bump `STATE_VERSION` when the persisted session shape changes.
- Only one UiAutomation client can run per device: while the helper runs, `uiautomator dump` fails.
  Stop it with `adb shell am force-stop dev.mdh.helper` when capturing fixtures with uiautomator.

## MCP server

- Tools are thin: parse parameters, call the session, return the same text as the CLI (`text()`
  helpers and `text` fields live in `mdh-control`, not in an entry point).
- Every tool's parameters must be a struct (object schema at the root); `crates/mdh-mcp/tests/tools.rs`
  checks this. Document parameters with doc comments — they become the schema descriptions agents read.
- Keep the tool count and descriptions small; every definition costs agent context on every turn.

## Sample app

- `examples/android-sample` is the end-to-end target: build with `./gradlew assembleDebug`, install with
  `mdh install`, then drive it with the CLI or MCP. When a behavior changes, run the affected scenario there
  and look at the actual output, not just the tests — most M1 bugs were found this way.
- Real captures from it (e.g. `fixtures/android/logcat/sample_*_api36.txt`) back the parser tests; prefer them over
  synthetic input.
- Keep its deliberate bugs (Troubles screen, `OverlapActivity`) deliberate; fix accidental ones.

## Commits and PRs

- Commit messages: imperative summary line (≤ 72 chars), blank line, body explaining why.
  No trailers.
- Keep changes scoped to one milestone item; update the milestone table in `docs/DESIGN.md` and
  `README.md` when a milestone item lands.
- `README.md` (English) and `README.zh-CN.md` (Simplified Chinese) are user-facing and must say the same
  thing: update both in the same change. Everything else in the repo is English only.
- Significant design decisions get an ADR in `docs/adr/` (never edit an accepted ADR; supersede it).
