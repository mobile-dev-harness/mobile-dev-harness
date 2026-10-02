# AGENTS.md

Guidance for coding agents working in this repository. Human contributors: see
[CONTRIBUTING.md](CONTRIBUTING.md). Architecture and roadmap: [docs/DESIGN.md](docs/DESIGN.md).

## Project

`mobile-dev-harness` (`mdh`) is a quality harness for coding agents on mobile (ADR-0009): **control**
drives the app; the **verification engine** runs flows and pluggable **check kinds** (functional, UI
consistency, performance) into one verdict; the **compatibility matrix** repeats flows and checks
across devices and configurations; all on a shared foundation. Exposed as a CLI and (from M1) an MCP server. Android
first; iOS, React Native and Flutter later.

**Current state: M0–M2 done, M4 in progress.** Implemented: the session engine in `mdh-control`
(session-stable refs, ref/selector/label targeting, wait-for-stable, diffs after actions, recorded
steps; CLI sessions in `.mdh/session.json`), `observe [--diff]`, `tap`, `long-press`, `type`,
`scroll`, `swipe`, `key`, `wait`, `screenshot`, `launch`/`stop`/`install`, `session show|reset`, `logs`;
log digests and crash reports on every observation (crash → exit 5); the MCP server (`mdh mcp`); the
on-device helper; the ADR-0005 output envelope; `examples/android-sample`; M2: `mdh run` / `mdh_run`
(Gradle probe, build, structured diagnostics, install-if-changed, restart, first observation); M4 so
far: change impact analysis in `mdh-impact` (`mdh impact` / `mdh_impact`, ADR-0010) and the
verification engine in `mdh-verify` (`Check` interface, functional checks, verdicts with evidence in
`.mdh/runs/`, flows saved from recordings and replayed with setup, JUnit; `mdh verify`, `mdh flow`,
`mdh_verify`, `mdh_flow`), the flows a change needs picked from its impact (`flow run --changed`),
state for flows (animations, permissions, data reset, deep links: `mdh state`, `mdh open`), the
Claude Code plugin (`integrations/claude-code`, hooks via `mdh hook`), `mdh init` (AGENTS.md
section), and the emulator e2e job replaying the sample's flows. Next: M5 (UI consistency checks).
State and config is a track that grows with each milestone; the benchmark is the last milestone (M10).
Feature IDs like `F4.1` refer to docs/design/01-functional.md.

## What we optimize for

Precise verification: fewer false passes and false fails, fewer tokens. Before adding or changing
a feature, check it against the design principles in [docs/DESIGN.md](docs/DESIGN.md#design-principles)
(facts over pixels, never a stale state, report the unasked, the harness judges, every token earns
its place, failures say what to do next).

## Layout

```
crates/
  mdh-core/      foundation: shared types, errors and codes, output envelope, timings — no I/O
  mdh-driver/    foundation: Driver trait + android/ (SDK, adb, helper client, uiautomator, parsers)
  mdh-observe/   foundation: compact UI trees, stable refs, rendering, diffs, screenshots; logs next
  mdh-project/   foundation: Gradle probe (init script), builds, diagnostics, APK lookup
  mdh-impact/    verification input: change impact — tree-sitter index of Kotlin/Java/Android XML, git
                 change set, declaration diff, users up to screens, what to verify (no device, no build)
  mdh-control/   control: session engine, targeting, actions, waiting, app lifecycle, `run`
  mdh-verify/    verification engine: Check interface, functional checks, verdicts and evidence, flows
                 (YAML via serde_norway behind `yaml.rs`), replay, JUnit
  mdh-visual/    check kind: UI consistency                                 — empty until M5
  mdh-perf/      check kind: performance                                    — empty until M6
  mdh-compat/    matrix: compatibility across devices and configurations   — empty until M7
  mdh-mcp/       entry point: MCP server over stdio (rmcp), compiled into `mdh mcp`
  mdh-cli/       entry point: package `mobile-dev-harness`, binary `mdh` (parsing + rendering only)
integrations/claude-code/  Claude Code plugin (MCP config, skills, hooks calling `mdh hook`); the
                 marketplace is .claude-plugin/marketplace.json at the root
android-helper/  on-device helper APK (Java, no dependencies); see docs/design/02-architecture.md §10
examples/android-sample/  test app exercising every feature (Kotlin, Views + Compose); its flows in
                 .mdh/flows/ are replayed on an emulator by .github/workflows/e2e.yml
scripts/         build-helper.sh rebuilds the helper into crates/mdh-driver/assets/
                 diagram/ generates docs/assets/architecture-{light,dark}.svg (Rough.js; `npm install && npm run build`)
fixtures/        real tool output used by tests (e.g. android/uiautomator/<screen>_api<level>.xml)
docs/DESIGN.md   design overview + roadmap; details in docs/design/, decisions in docs/adr/
```

Dependencies point strictly downward: entry points → compat → visual / perf → verify → control →
observe / project → driver → core; `mdh-impact` depends only on core and is used by verify and the
entry points. New kinds of checks (accessibility, security, …) implement the
engine's `Check` interface in their own crate instead of growing `mdh-verify`; compatibility is not a
check kind but runs flows and checks per matrix cell. Never make a lower crate depend on a higher one.
Crates for later milestones exist with their scope documented in `lib.rs`; keep them empty until
their milestone starts. Logic belongs in library
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

- **Rust edition 2024, MSRV 1.88** (the oldest toolchain the dependencies build on; CI checks it).
  Don't use std APIs or language features stabilized later, even if your local toolchain is newer.
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

## Verification engine

- Verification is tested against the scripted fake driver in `crates/mdh-verify/tests/verify.rs`
  (screens advance on each input); add a scenario for every behavior change.
- `no crash` must look at the whole window (session or flow), not only unreported logs: a crash the
  agent already saw must still fail the verdict.
- Screen checks poll until they hold; never add fixed sleeps to flows or checks.
- Flows of the sample app are an end-to-end test: when a change alters a sample screen, update the
  flow and run it locally (`MDH_PASSWORD=correct-horse mdh flow run …` in examples/android-sample).
- Run `claude plugin validate ./integrations/claude-code` and `claude plugin validate .` after
  changing the plugin or the marketplace.
- Verdict text is what agents read: one line per check, observed values only for failures, evidence
  as file paths (never inline screenshots).

## Impact analysis

- Syntax only (ADR-0010): never add a type checker, a Gradle call or a language server to
  `mdh-impact`; it must stay fast enough to run after every edit (Now in Android: ~150 ms).
- Extractors (`kotlin/`, `java.rs`, `xml.rs`) produce `FileIndex` values and are tested on source
  snippets; end-to-end behavior is tested on a temporary git repository in
  `crates/mdh-impact/tests/analyze.rs`, with the rendered text pinned by an `insta` snapshot.
- Link exactly one tree-sitter grammar per language: grammars export fixed C symbols
  (`tree_sitter_kotlin`), and two Kotlin grammars in one binary silently share one of them.
- Before changing resolution or propagation, run `mdh impact` on a real project with real edits (the
  sample app, and a large app such as Now in Android) and read the output; a wrong edge shows up as a
  screen that shouldn't be there.

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
- The README architecture diagram is code (`scripts/diagram/architecture.mjs`); when crates, domains or
  data paths change, update it and regenerate both SVGs rather than editing them.
- Significant design decisions get an ADR in `docs/adr/` (never edit an accepted ADR; supersede it).
