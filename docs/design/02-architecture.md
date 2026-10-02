# 02 · Technical Architecture

> Status: draft v0.2 · Companion docs: [01-functional.md](01-functional.md), [ADRs](../adr/)

## 1. Overview

```
             ┌──────────────┐      ┌──────────────┐
  agent ───▶ │   mdh-mcp    │      │   mdh-cli    │ ◀─── humans / CI / agents (shell)
             └──────┬───────┘      └──────┬───────┘
                    └──────────┬──────────┘
                        ┌──────▼──────┐
                        │ mdh-engine  │  session · orchestration · recording · output contract
                        └──────┬──────┘
     ┌──────────────┬──────────┼──────────┬──────────────┐
┌────▼─────┐ ┌──────▼────┐ ┌───▼────┐ ┌───▼──────┐ ┌─────▼─────┐
│mdh-build │ │ mdh-state │ │ mdh-ui │ │mdh-observe│ │mdh-verify │
│Gradle    │ │perms,     │ │compress│ │logs,      │ │assertions,│
│adapters, │ │snapshots, │ │diff,   │ │crashes    │ │flows,     │
│diagnostic│ │deep links │ │images  │ │           │ │verdicts   │
└────┬─────┘ └──────┬────┘ └───┬────┘ └───┬──────┘ └───────────┘
     └──────────────┴──────────┼──────────┘
                        ┌──────▼──────┐
                        │ mdh-driver  │  Driver trait · android (adb / helper) · ios (later)
                        └──────┬──────┘
                        ┌──────▼──────┐
                        │  mdh-core   │  types · errors · config model (no I/O)
                        └─────────────┘
```

Principles:

1. **CLI and MCP are thin shells.** All logic lives in `mdh-engine` and below, so both entry points behave
   identically.
2. **Platform-neutral logic is separated from platform specifics.** Compression, diffing, assertions and flows
   depend only on neutral data models; adb, uiautomator and Gradle details stay inside drivers and build adapters.
3. **Pure parsers with fixture tests.** Every parser of external tool output is `&str → struct`; CI needs no device.
4. **Output is product.** Agent-facing text and JSON are stable interfaces with a schema version.

## 2. Crates

| Crate | Responsibility | Depends on | Introduced |
|---|---|---|---|
| `mdh-core` | Neutral types (Device, AppId, Selector, Action, UiNode, …), errors, config model | — | M0 ✅ |
| `mdh-driver` | `Driver` trait; `android::{AdbDriver, HelperDriver}`; SDK discovery; process utilities | core | M0 ✅ |
| `mdh-ui` | Raw tree → compact tree, ref assignment, diff, opaque-region detection, image processing (resize, annotate, compare) | core | M1 |
| `mdh-observe` | logcat parsing, per-app filtering, crash/ANR detection and reports | core, driver | M1 |
| `mdh-engine` | `Session`, action execution with auto-observation, recording, run directories, output rendering | all of the above | M1 |
| `mdh-mcp` | MCP server (`rmcp`, stdio) | engine | M1 |
| `mdh-build` | `ProjectAdapter` trait; Gradle adapter: probing, building, diagnostic parsing | core | M2 |
| `mdh-state` | Animations, permissions, appearance, resets, snapshots, deep links, test data | core, driver | M3 |
| `mdh-verify` | Assertion model and evaluation, flow format, verdicts, JUnit output | core | M4 |
| `mobile-dev-harness` | CLI (`mdh`), the only published binary | engine, mcp | M0 ✅ |

`mdh-mcp` is compiled into the same `mdh` binary (`mdh mcp`) rather than shipped separately, so users install one thing.

## 3. Core abstractions

### 3.1 Neutral data model (`mdh-core`)

```rust
pub struct AppId(String);                 // Android applicationId / iOS bundle id

pub enum Artifact { Apk(PathBuf), Aab(PathBuf), AppBundle(PathBuf) /* iOS .app */ }

/// A backend-agnostic raw UI node. The adb backend converts uiautomator XML, the helper backend converts
/// JSON, the iOS backend converts the accessibility tree.
pub struct RawNode {
    pub class: String,                    // original class name, e.g. android.widget.Button
    pub resource_id: Option<String>,
    pub text: Option<String>,
    pub desc: Option<String>,             // content-desc / accessibilityLabel
    pub bounds: Rect,
    pub flags: NodeFlags,                 // clickable, focusable, scrollable, checked, enabled, password, selected...
    pub children: Vec<RawNode>,
}

pub struct Selector {                      // persistable, used by flows
    pub id: Option<String>,
    pub text: Option<TextMatch>,           // exact / contains / regex
    pub desc: Option<TextMatch>,
    pub role: Option<Role>,
    pub index: Option<usize>,              // position among multiple matches; last resort
    pub within: Option<Box<Selector>>,     // ancestor constraint
}

pub enum Target { Ref(String), Selector(Selector), Point(i32, i32) }

pub enum Action {
    Tap(Target), LongPress(Target, Duration),
    Type { target: Option<Target>, text: SecretOr<String>, clear: bool },
    Swipe { from: Point, to: Point, duration: Duration },
    ScrollUntil { container: Option<Target>, until: Selector, direction: Direction, max: u32 },
    Back, Home, Key(KeyCode), HideKeyboard,
    Wait { cond: WaitCond, timeout: Duration },
}
```

### 3.2 Driver (`mdh-driver`)

```rust
#[async_trait]
pub trait Driver: Send + Sync {
    fn platform(&self) -> Platform;
    fn capabilities(&self) -> Capabilities;   // e.g. unicode_input, fast_ui_tree, snapshots

    async fn devices(&self) -> Result<Vec<Device>>;
    async fn install(&self, dev: &Device, artifact: &Artifact, opts: InstallOpts) -> Result<()>;
    async fn launch(&self, dev: &Device, app: &AppId, opts: LaunchOpts) -> Result<LaunchInfo>;
    async fn stop(&self, dev: &Device, app: &AppId) -> Result<()>;
    async fn foreground(&self, dev: &Device) -> Result<ScreenInfo>;   // foreground screen, dialogs, keyboard
    async fn ui_tree(&self, dev: &Device) -> Result<RawNode>;
    async fn screenshot(&self, dev: &Device) -> Result<Image>;
    async fn input(&self, dev: &Device, input: LowLevelInput) -> Result<()>;  // coordinate-level tap/swipe/text/key
    async fn logs(&self, dev: &Device, query: LogQuery) -> Result<Vec<LogEntry>>;
    async fn shell(&self, dev: &Device, args: &[&str]) -> Result<String>;     // escape hatch for mdh-state
}
```

Notes:

- Drivers expose **low-level capabilities only** (coordinate input, raw trees). Ref/selector resolution, waiting
  and auto-observation live in the engine and are shared by every backend.
- `capabilities()` lets upper layers degrade explicitly: with `unicode_input = false`, typing Chinese fails with a
  hint to install the helper instead of silently typing garbage.
- Android has two implementations: `AdbDriver` (plain adb CLI, nothing to install) and `HelperDriver` (on-device
  helper, fast and complete). Capabilities the helper lacks fall back to `AdbDriver`.

### 3.3 ProjectAdapter (`mdh-build`)

```rust
#[async_trait]
pub trait ProjectAdapter: Send + Sync {
    fn kind(&self) -> ProjectKind;                       // Gradle, ReactNative, Expo, Flutter, Xcode
    async fn detect(dir: &Path) -> Result<Option<Self>> where Self: Sized;
    async fn model(&self) -> Result<ProjectModel>;       // modules, variants, app id, entry point, artifact paths
    async fn build(&self, req: &BuildRequest, progress: &dyn Progress) -> Result<BuildOutcome>;
}

pub enum BuildOutcome {
    Built { artifact: Artifact, duration: Duration, up_to_date: bool },
    Failed { diagnostics: Vec<Diagnostic>, log_path: PathBuf },
}
```

The RN, Expo and Flutter adapters reuse the Gradle adapter internally (their Android side is a Gradle project) and
only add the JS bundler, the Dart build and framework-specific logs.

### 3.4 Session (`mdh-engine`)

```rust
pub struct Session {
    device: Device,
    driver: Arc<dyn Driver>,
    app: Option<AppId>,
    refs: RefTable,              // stable key → ref, and ref → latest bounds and selector
    last_tree: Option<UiTree>,   // for diffs
    log_cursor: LogCursor,       // timestamp of the last log read
    recording: Vec<RecordedStep>,
    run_dir: RunDir,
}
```

- **MCP mode:** the session lives in memory for the lifetime of the MCP connection.
- **CLI mode:** loaded from `.mdh/session.json` on each invocation and written back at the end (ADR-0004: no
  resident daemon in the first release).
- Sessions are serializable, which also makes resuming after interruption and reproducing issues easy.

## 4. Key flows

### 4.1 `run`

```
load config ─▶ select device (boot the AVD and wait if needed)
           ─▶ adapter.model()   (skipped on cache hit)
           ─▶ adapter.build()   ──failure──▶ BUILD_FAILED + diagnostics (exit code 4)
           ─▶ artifact hash changed? ─yes─▶ driver.install()
           ─▶ state.apply(config.state)    # animations, permissions, reset
           ─▶ driver.launch() or navigate(route)
           ─▶ wait_stable() ─▶ observe() ─▶ output
```

### 4.2 Action with auto-observation

```
resolve target: ref → selector recorded in RefTable → match in the latest tree
                0 matches   → ELEMENT_NOT_FOUND + closest candidates (by text similarity)
                many matches → AMBIGUOUS_TARGET + candidate list
─▶ compute the tap point (center of the visible area, avoiding occluded parts)
─▶ driver.input()
─▶ wait_stable()   (see §6)
─▶ diff new tree against old + new logs + crash detection
─▶ record a RecordedStep (selector form)
─▶ output the diff observation
```

### 4.3 Flow replay

Uses the same executor as 4.2, except that targets are always selectors, each step first waits for its target
(with a timeout), assertions are evaluated by `mdh-verify`, and a verdict with evidence is produced at the end.
Recording and replay share one executor so that whatever was recorded can be replayed.

## 5. UI tree compression (`mdh-ui`)

Input: a `RawNode` tree. Output: a `UiTree` of compact nodes with refs.

1. **Prune:** drop zero-area nodes, nodes entirely off-screen, and nodes fully covered by opaque siblings (an
   approximation is fine).
2. **Infer roles:** map class names to a `Role` (button, textbox, checkbox, switch, image, list, item, tab, link,
   text, dialog, …), then correct with attributes, e.g. a clickable TextView becomes a button or link. For generic
   `android.view.View` nodes rendered by Compose or RN, rely mainly on attributes and semantics.
3. **Decide what to keep:** a node is kept if it is interactive (clickable, long-clickable, checkable, editable,
   scrollable), has text or a description, or is a semantic container (list, dialog, tab bar).
4. **Collapse:** a layout node with a single child and no information of its own is removed and its child lifted;
   plain text children inside an interactive node are merged into its label (e.g. the TextView inside a Button).
5. **Mask:** values of `password` nodes are always rendered as `••••`.
6. **Fold lists:** when structurally identical siblings in one container exceed a threshold, keep the first K
   visible ones and summarize the rest as "N more items".
7. **Stable refs:** each node gets a stable key = hash(role, resource_id, label, ancestor role path, index among
   siblings with the same key). Within a session, the same key reuses its ref; new keys get the next number. After
   an action, unchanged elements keep their numbers, which keeps diffs readable.
8. **Diff:** match the old and new trees by stable key and emit three kinds of change — added, removed, and
   attribute changes (text, checked, enabled, focused); contiguous removals merge into ranges (`- e1..e5`).
9. **Opaque-region detection:** if a large area of the screen (e.g. > 30%) contains no kept nodes, or a WebView is
   present, the observation marks `opaque_regions` and suggests a screenshot.

The compressor's output is pinned with snapshot tests (`insta`). Fixtures are uiautomator XML from real apps, and
the tests also record estimated token counts before and after compression to catch regressions.

## 6. Waiting and stability

Flakiness mostly comes from observing or acting while the UI is still changing. Countermeasures:

- **Disable animations at session start** (F3.1), removing the problem at its source.
- **wait_stable:** the UI counts as settled when two consecutive trees have the same structural hash (volatile text
  such as clocks excluded); polling backs off; there is an overall timeout. The helper backend uses
  `UiAutomation.waitForIdle` instead, which is faster and more accurate.
- **uiautomator "could not get idle state" failures:** retry, falling back to `--compressed` if needed.
- **System dialogs:** every observation checks whether the foreground window belongs to the system (permission,
  ANR or crash dialogs) and flags it separately so the agent deals with it first.
- Every wait has a timeout; timeout errors include the last observation so the agent can see where it got stuck.
- **Launches that don't navigate:** when the target app already has a task, `am start` may only bring it to the
  front and print `Warning: Activity not started, its current task has been brought to the front`. Launch and
  deep-link navigation must detect this and, depending on the reset policy, retry with `-S` (stop first) or
  report that navigation did not happen instead of claiming success.

## 7. Logs and crashes (`mdh-observe`)

- Collection: read incrementally with `logcat -v epoch,uid -T <cursor>` and filter by the app's uid. Filtering by
  uid rather than pid keeps up across process restarts; on older systems without uid support, fall back to pid
  and re-resolve it when the process restarts.
- Crash detection:
  - Java/Kotlin: `FATAL EXCEPTION` in the `crash` buffer, plus the `AndroidRuntime` tag;
  - Native: tombstone headers (`*** *** ***`) under the `DEBUG` tag;
  - ANR: `am_anr` in the `events` buffer, plus `ANR in <pkg>` from `ActivityManager`.
- Crash report: exception type and message, stack (app frames marked, framework frames folded), the `Caused by`
  chain, and the last N actions before the crash (from the session recording).
- Bounded output: an observation only carries counts and a few of the latest warning/error summaries; full logs
  go to the run directory.

> To verify during implementation on real devices across API levels: availability of `-v uid` / `--uid`, and all
> ANR signal sources.

## 8. Gradle probing and diagnostics (`mdh-build`)

### 8.1 Probing with an init script, not regexes

Regex-parsing `build.gradle(.kts)` is unreliable because of version catalogs, convention plugins, dynamic
applicationIds and so on. Instead, inject a Gradle init script:

```
./gradlew -q --init-script <mdh-probe.gradle> mdhProbe
```

For every project that applies `com.android.application`, the script uses AGP's `androidComponents.onVariants` to
collect variant names, `applicationId` and artifact locations, and prints them as JSON. Results are cached in
`.mdh/cache/`, keyed by a hash of all build files, `gradle.properties` and the wrapper version; on a cache hit
Gradle isn't started at all.

> Risk: AGP APIs differ across versions and need a version matrix (e.g. AGP 7.4, 8.x, 9.x). The launch activity is
> not part of the Gradle model; it is queried after install with `cmd package resolve-activity`.

### 8.2 Building and diagnostics

- Run `./gradlew :<module>:assemble<Variant>`, streaming output to `build.log` in the run directory.
- Diagnostic parsers (pure functions with fixture tests):
  - Kotlin: `e: file:///…/Foo.kt:12:5 Unresolved reference: bar`
  - Java: `Foo.java:12: error: …`
  - Resource and AAPT2 errors, manifest merger errors
  - Gradle's `* What went wrong:` section (dependency resolution, plugin and configuration errors)
- Output: the first N diagnostics in order (default 10), each with file, line, column and up to 3 lines of context;
  cascading errors after the first one are deprioritized when recognizable.

## 9. State and directories

- `mdh.yaml`: the config model is defined in `mdh-core`, deserialized with serde and validated; validation errors
  carry line numbers. The YAML crate is chosen in M3 (`serde_yaml` is unmaintained, so a well-maintained
  alternative is needed) and wrapped in an internal module so it can be swapped later.
- `.mdh/session.json`: CLI mode only, versioned; incompatible versions are discarded and rebuilt.
- `.mdh/runs/`: one directory per run; the latest 20 are kept (configurable).
- Restoring global settings: original values (e.g. animation scales) are recorded at session start and restored on
  `session reset` or when the MCP connection closes.

## 10. On-device helper (Android)

**Why:** `uiautomator dump` takes 1–3 s per call and fails easily; `adb shell input text` can't type Chinese or
other non-ASCII characters; and the adb backend has no efficient way to wait for idle. These are experience
bottlenecks, not details.

**Shape:** an instrumentation APK (Kotlin) started with `am instrument -w`, listening on a device-local port that the
host reaches through `adb forward`. Appium UiAutomator2 and Maestro both use this proven approach.

**Capabilities:**

- Accessibility tree straight from `UiAutomation`, returned as JSON (target < 200 ms)
- `waitForIdle`
- Unicode text input (`setText` on the focused node, or via an IME)
- Screenshots (`UiAutomation.takeScreenshot`)
- Window-change event stream (later, to push instead of poll)

**Project:** `android-helper/` in this repo, a standalone Gradle project. The APK ships with each release; `mdh`
downloads the matching version, verifies its SHA-256, and installs it automatically the first time it is needed.
The protocol is versioned JSON over TCP, kept simple.

**Timing:** the full helper is M5 on the roadmap; if M1 measurements show dump latency to be a blocker, it moves
earlier (see §15).

**Minimal input helper (M1):** Unicode input can't wait for M5 — mobile-mcp already supports it through its
DeviceKit APK, and lacking it would be a regression for apps with Chinese users. M1 ships a tiny APK with no
dependencies: a broadcast receiver that puts base64-decoded text on the clipboard, after which the driver sends
`KEYCODE_PASTE` and restores the previous clipboard content. It is installed on demand, and the full helper later
subsumes it behind the same `unicode_input` capability. Packaging (prebuilt APK checked into the repo with a
reproducible build script, embedded into the binary) is settled when the item is implemented.

## 11. MCP server (`mdh-mcp`)

- Built on `rmcp` over stdio; tool parameter JSON Schemas are generated from Rust types with `schemars`, so docs
  and implementation can't drift.
- Results are primarily text (the compact format from functional design §4.4) with structured JSON alongside
  (`structuredContent`); screenshots are returned as image content.
- Long operations (builds, emulator boot) send progress notifications.
- One MCP connection maps to one session; with multiple devices, tools take a device parameter.

## 12. Claude Code plugin

The repository doubles as a plugin marketplace:

```
integrations/claude-code/
  .claude-plugin/plugin.json
  .mcp.json                     # { "mdh": { "command": "mdh", "args": ["mcp"] } }
  skills/verify/SKILL.md        # protocol: run → navigate → observe → act → verify; not done without a pass verdict
  skills/debug-crash/SKILL.md   # crash triage protocol
  hooks/hooks.json              # SessionStart: inject `mdh status`; Stop: remind about unverified changes
.claude-plugin/marketplace.json # at the repo root, so users can add it directly with /plugin
```

"Unverified changes" means source files under `src/` or `res/` modified after the most recent pass verdict.

## 13. Error model

- Every `mdh_core::Error` variant maps to a stable `code` (`DEVICE_NOT_FOUND`, `BUILD_FAILED`,
  `ELEMENT_NOT_FOUND`, `AMBIGUOUS_TARGET`, `APP_CRASHED`, `TIMEOUT`, `CAPABILITY_MISSING`, `CONFIG_INVALID`, …)
  and **must carry a hint** telling the agent what to do next.
- Mapping: error code → exit code → `isError` in MCP results.
- Rule of thumb: **prefer one more piece of useful context over making the agent guess** — e.g. timeouts include
  the last observation, missing elements include candidates.

## 14. Concurrency and process management

- tokio runtime. All external processes start through one `process` module that handles timeouts, kill-on-drop
  and output size limits, so processes like logcat can't become orphans or exhaust memory.
- Actions on one device are serialized (a lock per device); different devices can run in parallel (groundwork for
  parallel flow replay across devices).
- Logs are read incrementally on demand in the first release; if real-time crash alerts are needed later, each
  session gets a background logcat task.

## 15. Technical risks

| Risk | Impact | Mitigation |
|---|---|---|
| `uiautomator dump` is slow and unreliable | High observation latency, flakiness | Disable animations + retries; measure real numbers in M1 and pull the helper forward if needed |
| No Chinese input | Form-based checks impossible for apps with Chinese users | Minimal input helper in M1 (clipboard + paste); full helper in M5 |
| Incomplete semantics in Compose, RN, WebView | Content invisible in the tree | Opaque-region detection + annotated screenshot fallback; docs guide developers to add `testTag`/`contentDescription` |
| AGP version differences | Probing fails | Version-matrix tests; allow manual override in config when probing fails |
| adb output differences across Android versions | Parse errors | Fixtures across API levels; tolerant parsers that ignore unknown fields |
| Rust raises the contribution barrier | Slower community growth | Clear crate boundaries, good-first-issues (e.g. adding a diagnostic parser), thorough fixture tests |

## 16. Observability

- Logging via `tracing`, enabled with `-v` or `MDH_LOG=debug`; logs go to stderr so stdout JSON stays clean.
- Every result carries per-phase `timing_ms`; the benchmark reuses these numbers.
- No telemetry of any kind.

## 17. Testing

| Level | What | When |
|---|---|---|
| Unit | Parsers, compression, diff, assertion evaluation, flow parsing | Every CI run |
| Snapshot | Compressor and renderer output (`insta`) | Every CI run |
| Integration | Engine on a `FakeDriver` (replays recorded tree and log sequences) | Every CI run |
| E2E | Real emulator + `examples/android-sample` | Linux KVM CI job (main and releases); locally with `MDH_E2E=1` |
| Benchmark | Latency, tokens and success rate on a set of standard tasks | Before releases; results published in README |

`examples/android-sample` is designed to exercise every feature: View and Compose screens, a login form, a long
list, a WebView, deep links, runtime permissions, a button that crashes, a button that causes an ANR, and Chinese
text.

## 18. Release and distribution

- **Versioning:** semver, 0.x for now; JSON output carries a `schema` version and changes are called out in the
  CHANGELOG.
- **Binaries:** GitHub Releases built with `cargo-dist` (macOS arm64/x64, Linux x64/arm64, Windows x64), with shell
  and PowerShell installers.
- **Channels:** crates.io (`cargo install mobile-dev-harness`), a Homebrew tap; later an npm wrapper
  (`npx mobile-dev-harness mcp`, convenient in MCP configs).
- **Helper APK:** released with the same version as `mdh`, downloaded on demand and verified at runtime.
- **Claude Code plugin:** distributed through the marketplace in this repository.

## 19. Security

- Only the target app is touched by default. Changes to other packages or global settings are either on an
  allowlist (animations, locale, dark mode) and restored at session end, or require an explicit flag.
- Destructive operations on physical devices require `--allow-device-changes`.
- Secrets are only referenced through env variables and are redacted in output, recordings and run directories.
- The helper only listens on the device's loopback interface, reachable through adb forward, never the network.
