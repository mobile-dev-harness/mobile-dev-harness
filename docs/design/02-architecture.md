# 02 · Technical Architecture

> Status: draft v0.5 · Companion docs: [01-functional.md](01-functional.md), [ADRs](../adr/)

## 1. Overview

```
            agent ──▶ mdh-mcp            mdh-cli ◀── humans / CI / agents (shell)
                         └───────┬───────────┘
          ┌──────────────────────▼──────────────────────────────────────┐
          │ mdh-compat   compatibility: the same flows and checks on the │   where it is checked
          │              versions, devices, vendors, sizes at risk       │
          │  ┌──────────────────────────────────────────────────────┐   │
          │  │ mdh-verify   verification engine                      │   │
          │  │   flows · check interface · verdicts · evidence ·     │   │
          │  │   baselines · reports                                 │   │
          │  │   what to verify ◀── mdh-impact (static, no device)   │   │
          │  │   ┌────────────┐ ┌──────────────┐ ┌───────────────┐   │   │   what is checked
          │  │   │ functional │ │ UI consist.  │ │ performance   │   │   │
          │  │   │ (built in) │ │ mdh-visual   │ │ mdh-perf      │   │   │
          │  │   └────────────┘ └──────────────┘ └───────────────┘   │   │
          │  └──────────────────────────┬───────────────────────────┘   │
          └─────────────────────────────┼───────────────────────────────┘
                         ┌──────────────▼──┐
                         │  mdh-control    │   sessions · targeting · actions · waiting · state · navigation
                         └──┬───────────┬──┘
               ┌────────────▼─┐    ┌────▼─────────┐
               │ mdh-observe  │    │ mdh-project  │   UI trees, logs, crashes, screenshots │ builds
               └──────┬───────┘    └──────┬───────┘
                      └────────┬──────────┘
                        ┌──────▼──────┐
                        │ mdh-driver  │   Driver trait · android (adb + on-device helper) · ios (later)
                        └──────┬──────┘
                        ┌──────▼──────┐
                        │  mdh-core   │   types · errors · config · output contract (no I/O)
                        └─────────────┘
```

Principles (the product's design principles — facts over pixels, never a stale state, the harness judges, … —
are in [DESIGN.md](../DESIGN.md#design-principles); these are the architectural ones):

1. **Entry points are thin shells.** CLI and MCP only parse input and render output; all logic lives in the
   library crates, so both behave identically.
2. **Control, then verification, then the matrix (ADR-0009).** Verification is an engine that owns flows and
   verdicts; functional, UI consistency and performance checks plug into it through one interface; the
   compatibility matrix repeats flows and checks across devices. Dependencies only point downward:
   `mdh-compat` → `mdh-visual`, `mdh-perf` → `mdh-verify` → `mdh-control` → foundation.
3. **Platform-neutral logic is separated from platform specifics.** Compression, diffing, assertions, flows, perf
   statistics and visual comparison depend only on neutral data models; adb, uiautomator, dumpsys and Gradle details
   stay inside the driver and project adapters.
4. **Pure parsers with fixture tests.** Every parser of external tool output is `&str → struct`; CI needs no device.
5. **Output is product.** Agent-facing text and JSON are stable interfaces with a schema version.

## 2. Crates

| Crate | Layer | Responsibility | Status |
|---|---|---|---|
| `mdh-core` | foundation | Neutral types (Device, RawNode, ScreenInfo, Input, LaunchInfo, …), errors and codes, output envelope and timings, config model | ✅ |
| `mdh-driver` | foundation | `Driver` trait; Android: SDK discovery, adb wrapper, on-device helper client, uiautomator fallback, dumpsys/am parsers | ✅ |
| `mdh-observe` | foundation | Compact UI trees (compression, roles, stable keys, refs, rendering, diffs, opaque regions), screenshot processing; logs and crash reports next | ✅ partly |
| `mdh-project` | foundation | Gradle probing, building, diagnostics, APK selection; later RN, Expo, Flutter, Xcode | ✅ |
| `mdh-control` | control | Device selection, observation, input, app lifecycle, session engine, `run` ✅; state setup, navigation | M1–M3 |
| `mdh-impact` | analyzer | Change impact: tree-sitter index of Kotlin, Java and Android XML, git change set, declaration diff, users up to screens, what to verify, the facts compatibility needs; no other mdh crate (§23) | ✅ |
| `mdh-risk` | analyzer | Compatibility risks of a change: rules over impact's facts and the vendored knowledge base (`compat-kb`); depends only on `mdh-impact` (§12) | ✅ |
| `mdh-verify` | verification engine | `Check` interface, verdicts, evidence, flow save/replay, JUnit reports, functional checks ✅ (§24); baselines next | M4 |
| `mdh-visual` | check kind | UI consistency: rule checks, structural and pixel baselines, contrast, cross-config layout checks ✅; design mocks next (§13) | M5 ✅, M8 |
| `mdh-perf` | check kind | Performance: startup, frames, memory, CPU, budgets and baselines; Perfetto traces explain regressions | M6 ✅ |
| `mdh-compat` | orchestrator | Compatibility (ADR-0011): verification plans from `mdh-risk`'s risks, device pool and providers, config application, verdicts per risk | M7 ✅, M8 |
| `mdh-mcp` | entry | MCP server (`rmcp`, stdio), compiled into the `mdh` binary | ✅ |
| `mobile-dev-harness` (`crates/mdh-cli`) | entry | CLI `mdh`, the only published binary | ✅ |

`android-helper/` (Java) is the on-device half of `mdh-driver` (§10).

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
    async fn devices(&self) -> Result<Vec<Device>>;
    async fn ui_tree(&self, device: &Device) -> Result<RawTree>;          // roots + windows + source
    async fn foreground_activity(&self, device: &Device) -> Result<Option<String>>;
    async fn input(&self, device: &Device, input: &Input) -> Result<()>;  // tap / swipe / key / set_text
    async fn screenshot(&self, device: &Device) -> Result<Vec<u8>>;       // PNG
    async fn install(&self, device: &Device, app: &Path, grant_permissions: bool) -> Result<()>;
    async fn launch(&self, device: &Device, app: &str) -> Result<LaunchInfo>;
    async fn stop(&self, device: &Device, package: &str) -> Result<()>;
}
```

Notes:

- Drivers expose **low-level capabilities only** (coordinate input, raw trees). Ref and selector resolution,
  waiting and auto-observation live in `mdh-control` and are shared by every backend.
- The Android driver prefers the on-device helper (§10) and falls back to `uiautomator dump` and `adb shell input`
  when it can't run; text input requires the helper and fails explicitly without it.
- Logs, perf counters (`dumpsys gfxinfo`, `meminfo`) and device configuration (`settings`, `wm`, `cmd uimode`) will
  be added as further driver capabilities when their domains start, rather than through a generic shell escape.

### 3.3 ProjectAdapter (`mdh-project`)

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

### 3.4 Session (`mdh-control`)

`Control` wraps a driver and a device with stateless operations (snapshot, input, screenshot, app lifecycle).
`Session` adds the state that makes consecutive calls one conversation:

| State | Purpose |
|---|---|
| `refs: RefTable` | stable key → ref; the same element keeps its ref for the whole session, also across screens |
| `last: View` | tree and screen the agent saw last; diffs are relative to it, and stale refs are re-found through it |
| `seen` | ref → what it was and on which screen, to explain refs that are no longer on screen |
| `steps` | every action with refs replaced by selectors (`selector_for`), replayable in another session |

- **MCP mode:** one session in memory per connection.
- **CLI mode:** loaded from `.mdh/session.json` (working directory) on each invocation and written back at the end
  (ADR-0004: no resident daemon). A file from another device or state version is discarded.
- `session reset` clears the state and stops the device helper, freeing UiAutomation for other tools.

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
fresh snapshot → assign session refs
resolve target:  ref → current tree; missing → selector derived from the agent's last view → current tree
                 selector → all matches; several matches but exactly one control → the control
                 0 matches → ELEMENT_NOT_FOUND + closest candidates (text similarity) or where the ref was seen
                 several → AMBIGUOUS_TARGET + candidates
─▶ driver.input() at the center of the element's visible bounds
─▶ settle (§6)
─▶ report against the agent's last view:
     activity changed, or added + removed > 60% of both trees → full tree (new screen)
     otherwise → diff (+ added, ~ changed, - removed) or "no visible change", plus opaque regions
─▶ record the step (selector form) → output
```

### 4.3 Flow replay

Uses the same executor as 4.2 (`Session::act`), except that targets are always selectors, each step first waits
for its target (with a timeout), checks are evaluated by `mdh-verify`, and a verdict with evidence is produced at
the end. Recording and replay share one executor so that whatever was recorded can be replayed. Details in §24.

## 5. UI tree compression (`mdh-observe`)

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

Further rules learned on `examples/android-sample` (all covered by tests):

- **Role descriptions first.** Toolkits announce roles in `AccessibilityNodeInfo.roleDescription` when the class is
  generic; a Compose `toggleable(role = Role.Switch)` row is a checkable `View` whose description sits on a child
  with the same bounds.
- **Text field labels from children.** Compose renders an `OutlinedTextField` label as a child text, so a field
  without hint or description takes its first text child as label.
- **The app's own overlays.** Containers drawn after (on top of) a node that have content and cover less than half
  the screen — action bars, toolbars, bottom bars — are recorded per node (`covered_by`). Together with system
  windows they decide the tap point (center of the largest visible part) and the `obscured` state (less than half
  visible). Observed with an edge-to-edge app whose first buttons sat under the action bar.
- **WebViews** are opaque only when empty: the system WebView exposes the page through accessibility. Directly
  nested WebView nodes collapse.
- **Spacers aren't opaque.** Empty generic views and layouts (`View`, `Space`, `FrameLayout`, …) are spacers or
  backgrounds; only custom views, `SurfaceView`, `TextureView` and images count as drawn content.

The compressor's output is pinned with snapshot tests (`insta`). Fixtures are uiautomator XML from real apps, and
the tests also record estimated token counts before and after compression to catch regressions. One screen is
also kept as the on-device helper answers it (`fixtures/android/helper/`), windows included, to pin what the
status bar obscures.

## 6. Waiting and stability

Flakiness mostly comes from observing or acting while the UI is still changing. Countermeasures:

- **Disable animations at session start** (F3.1), removing the problem at its source.
- **wait_stable:** the UI counts as settled when two consecutive trees have the same structural hash (volatile text
  such as clocks excluded); polling backs off; there is an overall timeout. With the helper a tree costs ~10 ms,
  so this check is cheap.
- **`waitForIdle` alone is not enough (observed):** right after an action it can return immediately, because
  accessibility events are throttled (~100 ms) and the action's events haven't been delivered yet. After an action
  wait_stable therefore (1) waits a minimum settle time longer than the throttle (150 ms), (2) calls `waitForIdle`
  (200 ms quiet, at most 2 s), and (3) confirms with two identical consecutive tree fingerprints (80 ms apart,
  5 s overall timeout).
- **Windows that move:** the keyboard slides in and out over an app that doesn't resize for it, so the tree is
  the same in every frame. The two consecutive reads must also list the same windows in the same places;
  otherwise an action returned with the keyboard halfway (observed on Now in Android's search screen, 3 of 10
  actions), its `keyboard` flag and obstructions already out of date.
- **Measured** on API 36: a whole action takes 0.8–1.4 s, of which input is ~40 ms and settling the rest — mostly
  `waitForIdle` waiting out real transition animations, which is time the UI genuinely needs.
- **Spinners:** indeterminate progress indicators animate without accessibility events or tree changes, so a tap
  that starts loading would settle on the spinner (observed on the sample's sign-in). Settling also waits while a
  progress node is showing that wasn't there before the action, within the 5 s timeout; spinners present before
  the action (e.g. "searching for networks") are ignored.
- **Unresponsive apps:** while the app's main thread is blocked, each tree read blocks inside the helper (~10 s
  observed). Settling abandons a read after 2 s and reports that the app did not respond — before the system
  declares an ANR, which needs an input event during the block.
- **Scrolling without fling:** scroll gestures (400 ms) hold still at the end for 150 ms before lifting, so lists
  stop where the finger stopped; a quick swipe flung `scroll --until` 30+ rows past its target.
- **No active window:** briefly none exists while an app dies or windows change; the helper then falls back to the
  top-most application window, and an empty screen yields an empty tree instead of an error.
- **uiautomator "could not get idle state" failures:** retry, falling back to `--compressed` if needed.
- **System dialogs:** every observation checks whether the foreground window belongs to the system (permission,
  ANR or crash dialogs) and flags it separately so the agent deals with it first.
- Every wait has a timeout; timeout errors include the last observation so the agent can see where it got stuck.
- **Waiting for something to use:** an element entirely under a system window is in the tree (obscured), but
  `wait`, a flow step waiting for its target and `scroll --until` only count it once part of it is clear: the
  keyboard may be on its way out, a row may have only just scrolled in under a bar. `wait --gone` counts it as
  still there. A timeout or the end of the list says that it is covered, not that it is missing.
- **Launches that don't navigate:** when the target app already has a task, `am start` may only bring it to the
  front and print `Warning: Activity not started, its current task has been brought to the front`. Launch and
  deep-link navigation must detect this and, depending on the reset policy, retry with `-S` (stop first) or
  report that navigation did not happen instead of claiming success.
- **Starts that die:** `pm clear` returns while the app's tasks are still closing. Since Android 14, a task that
  another app's activity was on top of (a permission dialog) keeps a kill pending until that activity is
  destroyed, a second later at most; it then takes every process of the app that has no activity attached, the
  one a start has just created included (`remove task` in the log, no crash). `am start -W` reports a start
  without a `TotalTime` after ten seconds, or never returns, and the launcher is in front. Seen in about one of
  15 flows that reset an app showing its permission dialog. Clearing an app's data therefore returns only once
  `am stack list` no longer lists a task of the app (half a second when the app was in front, a second under a
  dialog). Starting again instead would start an app that crashes on launch twice: `am` reports that the same
  way.

## 7. Logs and crashes (`mdh-observe`)

- **Collection:** `logcat -d -v epoch -v uid -b main,system,crash -T <cursor>` (capped to the newest 5,000 lines),
  parsed by a pure function in the driver. Short tags are padded (`CCodec  :`), and messages keep their leading tab
  (stack frames).
- **Cursor:** device clock (`date +%s.%N`), never the host clock; the first observation of a session only starts
  it, so a session never reports the device's history. Each observation, action and wait reports what arrived
  since the previous one and advances the cursor, so every crash is reported exactly once.
- **Whose logs:** filtered by **pid, not uid** — system apps such as Settings run as uid 1000 together with
  system_server (observed), so a uid filter would include the whole system. Pids come from `pidof` of the
  session's app (set by `launch`), the app in front before the action and the app in front now, plus pids
  announced in the logs (`ActivityManager: Start proc <pid>:<package>`), so restarts are followed.
- **Crash detection** (any app, flagged `of_app` when it is one of the watched packages):
  - Java/Kotlin: an `AndroidRuntime` block starting with `FATAL EXCEPTION` (`Process: <pkg>, PID: <n>`, the
    exception, frames, `Caused by:` chain);
  - native: crash_dump's `DEBUG` block (`*** *** ***`, `pid: … >>> <pkg> <<<`, `signal …`, `#NN pc …` frames);
  - ANR: `ActivityManager: ANR in <pkg>` followed by `PID:` and `Reason:`;
  - unexplained death: `Process <pkg> (pid <n>) has died` without a crash for that pid.
- **Report:** kind, package, pid, summary, the app's own frames (framework frames folded; the first frames when
  the stack has none of the app's), cause chain, and the last three session steps. A crash or ANR of the app makes
  the command fail with `APP_CRASHED` (exit 5) while still returning the observation.
- **Bounded output:** observations carry warning/error counts and the latest three distinct lines (repeats counted
  as `(×N)`); `mdh logs` shows the last N lines of the watched apps over ten minutes.
- **Measured:** reading and digesting logs adds ~80 ms to an action.

> Verified on API 36 with `am crash` (Java). Native crashes and ANRs are covered by synthetic fixtures until the
> sample app can produce real ones.

## 8. Gradle probing, building and diagnostics (`mdh-project`)

### 8.1 Probing with an init script, not regexes

Regex-parsing `build.gradle(.kts)` is unreliable because of version catalogs, convention plugins, dynamic
applicationIds and so on. Instead, mdh injects a Groovy init script (`crates/mdh-project/src/gradle/probe.gradle`):

```
./gradlew -q --init-script .mdh/cache/mdh-probe.gradle --no-configuration-cache --no-configure-on-demand mdhProbe
```

For every project applying `com.android.application`, the script registers an `androidComponents.onVariants`
callback through dynamic calls (the init script's classpath has no AGP) and records each variant's name, build
type and application id; the `mdhProbe` task prints them as one JSON line. Configuration cache and
configure-on-demand are turned off for the probe so every module is configured. The model is cached in
`.mdh/cache/gradle-model.json`, keyed by a hash of every settings, build, `gradle.properties`, version catalog and
wrapper file, so Gradle only runs when one of them changes.

Verified on AGP 7.4.2, 8.7.3 and 9.2.1 (Gradle 7.6.4, 8.9, 9.4.1) with a single-module app and a two-module build
with product flavors: library modules are skipped and `applicationIdSuffix` is applied. AGP before 7.0 has no
`androidComponents` and isn't supported.

Selection: the only application module, or `--module`; the `debug` variant, or the only variant of build type
debug, or `--variant`. Several candidates are an `AMBIGUOUS_BUILD_TARGET` listing them (exit 2). Several modules
come with the variant each would build, or with those to choose from
(`:app (demoDebug or prodDebug), :app-nia-catalog (debug)`), so that the next call can name module and variant
instead of failing once for each.

### 8.2 Building and locating the APK

- `./gradlew <module>:assemble<Variant> --console=plain` (the wrapper when present), stdout and stderr interleaved
  line by line into `.mdh/runs/<time>-build/build.log` (the latest 20 are kept). `> Task` lines feed progress:
  the CLI shows the running task on a terminal, the MCP server sends progress notifications (at most two per
  second) when the client provides a progress token.
- The APKs and their application id come from AGP's `output-metadata.json` under `<module>/build/outputs/apk/`
  (format version 3 on all three AGP versions), not from guessing output paths. With ABI splits there is one APK
  per ABI plus maybe a universal one; `run` installs the split for the device's preferred ABI
  (`ro.product.cpu.abilist`), else the universal APK, else fails with `NO_APK` naming both sides. Taking the first
  element would install an x86_64 split on an arm64 device (`INSTALL_FAILED_NO_MATCHING_ABIS`, observed).
- When neither `ANDROID_HOME` nor `ANDROID_SDK_ROOT` is set, Gradle gets the SDK mdh found as `ANDROID_HOME`, so a
  project without `local.properties` still builds (`sdk.dir` still wins when present).
- "Up to date" means Gradle's summary reports every task up to date.

### 8.3 Diagnostics

Parsers are pure functions over the captured output, tested with real failures of the sample app
(`fixtures/android/gradle/`). Formats are recognized by shape, not English keywords:

| Kind | Format | Notes |
|---|---|---|
| Kotlin | `e: file:///…/Foo.kt:29:9 message` (`w:` for warnings) | the source line is read from disk |
| Java | `/…/Foo.java:5: <severity>: message`, source line, caret, indented details | **javac is localized** (`错误:` with a Chinese locale, observed); the severity word is matched against known warnings, anything else is an error |
| Resources | `dev.mdh.sample-main-38:/layout/x.xml:33: error: …` | the prefix is a resource set (`<package>-<source set>-<n>`), mapped back to `src/<source set>/res/…` |
| Manifest | `/…/AndroidManifest.xml Error:` then tab-indented message and suggestions | suggestions are kept as notes |
| Dependencies | `Could not find group:artifact:version.`, repeated once per dependency path | reported once, with the first `Required by` |
| Duplicate classes | `Duplicate class X found in modules a (g:a:1) and b (g:b:2)`, once per class | one diagnostic per pair of artifacts with a count; Jetifier hint when one side is the old Support Library |
| KSP / kapt | `e: [ksp] /…/Foo.kt:12: message` | the prefix is stripped, then parsed like Kotlin |
| Anything else | the `* What went wrong:` paragraph | only when nothing more specific explains the failure; a hint for known environment causes (SDK location, JDK version, SDK licenses, unreachable repositories) |

Gradle repeats compiler output under `* What went wrong:`; duplicates are dropped. Paths are reported relative to
the project root. The report shows up to eight errors, each with its source line, and points to the full log.

### 8.4 `run`

Probe (cached) → select → build → choose the APK for the device's ABI → install with `-r -t -d` (`-d` lets debug
builds go back in version) unless this exact APK is what `run` installed last *and* the device still has that
install (`pm path` changes with every install, so an install by someone else is noticed) → force-stop and launch, with the log cursor started first so a crash during startup is
reported → settle (splash screens, first loads) → observe. A failed build stops after the build report and
returns `BUILD_FAILED` (exit 4) next to it; a later failure reports the steps that succeeded and then the error.

Install failures are parsed from `Failure [INSTALL_FAILED_…: detail]` (adb may print a failed incremental attempt
first; the last failure counts) into `INSTALL_FAILED` (exit 3) with a fix per code: another signing key or a
newer version → `--reinstall` (uninstalls first, clearing the app's data, so never implicit); missing ABI;
minSdk too high; no space; installs blocked or awaiting confirmation on vendor ROMs; unsigned APKs.

Measured on the sample app: first run 6.3 s (probe 0.9, build 2.7, install 0.8, launch 1.5); nothing changed:
build "up to date" in 0.8 s and the install skipped; a one-line edit to the visible screen 3.2 s end to end.

## 9. State and directories

- `mdh.yaml`: the config model is defined in `mdh-core`, deserialized with serde and validated; validation errors
  carry line numbers. YAML is read and written with `serde_norway` (the maintained fork of the unmaintained
  `serde_yaml`; `serde-saphyr` needs a newer Rust than the MSRV), behind `mdh-verify`'s `yaml` module so it can be
  swapped; enums are written as single-key maps (`- tap: Sign in`), not YAML tags.
- `.mdh/session.json`: CLI mode only, versioned; incompatible versions are discarded and rebuilt.
- `.mdh/runs/`: one directory per run; the latest 20 are kept (configurable).
- `.mdh/baselines/`: visual (`visual/<scope>/<checkpoint>/<profile>.*`) and performance
  (`perf/<profile>/<scope>.json`) baselines, committed; candidates (`*.new.*`) wait for `approve`.
- Host cache: Perfetto's trace processor in `~/Library/Caches/mdh` or `$XDG_CACHE_HOME/mdh` (downloaded only with
  the user's consent).
- Restoring global settings: original values (e.g. animation scales) are recorded at session start and restored on
  `session reset` or when the MCP connection closes.

## 10. On-device helper (Android)

**Why:** `uiautomator dump` costs ~2 s per call regardless of screen complexity (it starts a process and connects
a new UiAutomation session each time), `adb shell input text` can't type non-ASCII text, and the adb backend has no
efficient way to wait for idle. Measurements and the decision to ship it in M1 are in ADR-0007.

**Shape:** a dependency-free Java instrumentation APK (`android-helper/`, ~10 KB) that targets itself, since
UiAutomation is device-wide.

- The UiAutomation connection is hosted by the `am` process, so the host starts it with
  `nohup am instrument -w dev.mdh.helper/.HelperInstrumentation &` on the device: `-w` keeps `am` alive and `nohup`
  lets it outlive the adb session. The instrumentation never finishes, so the connection stays warm across mdh
  invocations.
- It listens on the abstract Unix socket `mdh-helper` (no permission, no port). The host reaches it through
  `adb forward tcp:0 localabstract:mdh-helper`; forwards are owned by the adb server and reused across invocations.
- Protocol: one JSON request per line, one JSON response per line, `{"ok": true, ...}` or
  `{"ok": false, "error": ...}`. Commands: `ping` (version code, SDK level), `tree` (field names match `RawNode`),
  `wait_idle`, `set_text`, `tap`, `swipe` (with an optional `hold_ms` at the end), `key`; `tree` also returns the
  window list (keyboard, system bars, dialogs) and each node's role description.
- The tree is what the app shows, as `uiautomator dump` reports it: children that aren't visible to the user are
  skipped. Listing windows (`FLAG_RETRIEVE_INTERACTIVE_WINDOWS`) changes what that means: Android then also
  reports a node as not visible when it lies entirely under the windows above its own, whatever the app draws
  there. Such a node is kept when the windows over it are the system's (status bar, navigation bar, keyboard: the
  host's obstructions), so the host marks it `obscured` instead of losing it; observed with a back button drawn
  under the status bar. What can't be told apart stays wrong in the rare case: a node the app itself hides
  (alpha 0) that also lies entirely under a system window is reported. A node reaching beyond the display (a
  panned window) is still skipped, until the host clips to the display.
- Input is injected **asynchronously** through `UiAutomation.injectInputEvent`. Synchronous injection waits until
  the target window has handled the event and was observed to take 0.4–1.7 s while the app animates; settling is
  the host's job (§6).
- `Helper::ensure` (host): reuse the forward → ping → done if the version this binary embeds answers; otherwise
  stop the helper, install the embedded APK unless the installed `versionCode` is already that one, start it, poll
  until it answers. Host and helper change together, so only that version will do. An older helper is upgraded in
  place. One that can't be replaced in place is uninstalled first (it keeps no data): a newer one, installed by
  another mdh (Android refuses a downgrade, and `adb install -d` lifts that only for debuggable packages or system
  images), or one signed with another key (a local debug build). If Android still refuses the install, the command
  fails with `HELPER_UNAVAILABLE`, Android's reason and what to do, instead of falling back to `uiautomator dump`:
  the problem doesn't go away by itself, and slower, poorer observations would only hide it.
- Releases: `scripts/build-helper.sh` rebuilds the APK into `crates/mdh-driver/assets/`. The version code lives in
  three places (Gradle, `Commands.VERSION_CODE`, `HELPER_VERSION_CODE`) and must be bumped together.

**Measured:** warm `mdh observe` ~23 ms end to end; cold start ~360 ms; restart after a kill ~300 ms; tree ~7 ms
and tap ~36 ms (median) per helper request vs. ~120 ms for `adb shell input`. The foreground activity comes from
`dumpsys window displays` (~37 ms), fetched concurrently with the tree.

**Coexistence:** only one UiAutomation client can run per device. While the helper runs, other clients'
`uiautomator dump` is killed, so mobile-mcp, Appium or Maestro on the same device conflict with it. If the helper
can't start, mdh falls back to `uiautomator dump`.

**Two mdh versions on one device** (checkouts with different helpers, an old binary next to a new one) take turns:
each replaces the other's helper when it runs, which ends the other's UiAutomation connection until that mdh runs
again and does the same. A switch takes about a second instead of the ~23 ms of a warm call (0.8–1.0 s measured for
replacing a newer helper on the API 36 emulator), on every call if the calls alternate. Having the older mdh fail
with a hint instead was rejected: a newer mdh replaces an older helper anyway, so only the older one would be left
unable to read the screen, until someone uninstalled the helper by hand, which is the same replacement. Using the
newer helper as it is was rejected too: nothing says its answers mean what this host expects. The version is
checked when a process first uses a device and after a request fails, not on every request: a long-running mdh (the
MCP server, a flow) whose helper another version replaced keeps talking to that one until then.

**Later (M5):** window-change event stream (push instead of poll), screenshots through `UiAutomation`.

## 11. Performance checks (`mdh-perf`)

A check kind (ADR-0009) with its own runner: a measurement needs repetitions, so `mdh perf` (`mdh_perf` over MCP)
drives the engine instead of riding along with every verification. `perf startup` launches the app itself;
`perf flow` replays a saved flow N times through `run_flow` with a `Performance` check (the sampler) attached:
its `begin` hook, called after the flow's setup, resets the frame counters and notes the process's CPU time, and its
run at the `final` checkpoint reads frames, memory and CPU. The result is an ordinary verdict whose findings are the
metrics; a run that fails functionally stops the measurement and its verdict is returned instead.

**Data sources** (all via adb, parsed by pure functions with fixtures in `fixtures/android/`):

| Metric | Source |
|---|---|
| Cold, hot start | `am start -W` (`TotalTime`, `LaunchState`) after `force-stop` (cold) or HOME (hot) |
| Frames | `dumpsys gfxinfo <pkg> reset` at the start, `dumpsys gfxinfo <pkg>` at the end: janky-frame share, p90 and p99 frame time |
| Memory | `dumpsys meminfo <pkg>`: total PSS (Java heap, native heap and graphics are parsed too) |
| CPU | utime + stime of `/proc/<pid>/stat` (10 ms ticks) over the flow's wall time |

**Measurement protocol.** One extra first run is discarded (the first launch after an install pays for dex2oat and
cold caches). N measured runs (5 by default, or the flow's `perf: runs`) report median, p90 and median absolute
deviation (MAD). Animations stay on — jank can't be measured without them — so a session that turned them off has
them back on for the measurement and off again after it. Runs are separated by an 800 ms cool-down.

**Baselines and regressions.** `.mdh/baselines/perf/<profile>/<scope>.json`, committed with the project: the
profile is the AVD (or model), API level and build type (`Pixel_9_Pro_XL-api36-debug`; `run-as` tells debug
builds, whose numbers only compare with other debug builds and get a warning saying so); the scope is the flow or
`startup-<package>`. A regression needs the median to exceed the baseline's by more than three times the larger MAD
*and* by a per-metric minimum (cold start 50 ms and 5%, hot start 20 ms and 10%, janky frames 2 points, p90 4 ms and
10%, p99 8 ms and 25%, PSS 5 MB and 10%, CPU 5 points and 15%), so emulator jitter doesn't fail verdicts and a
noisy run can't show a small change. The first measurement records the baseline (a warning: commit it); later ones
write `<scope>.new.json`, which `mdh perf approve [scope]` promotes. Budgets (`perf: budgets:` in a flow, keyed
like the metrics: `cold_start_ms`, `janky_pct`, `frame_p90_ms`, …) are upper limits on the median, meant for
physical devices. PSS that grows with every run of the same flow by 10% (at least 5 MB) is reported as a leak
signal.

What gfxinfo can't see: frames that are never produced. Work between frames (RecyclerView prefetch, say) delays
or skips frames without making a counted frame slow; it shows as CPU, and the trace below names it.

**Traces.** On a regression, a budget exceeded or `--trace`, one more run is traced with Perfetto (Android 9+ ships
it): scheduling, `am wm gfx view dalvik binder_driver res input` atrace categories plus the app's own sections,
FrameTimeline (Android 12+), process renames. The config is written to `/data/local/tmp` and piped to `perfetto
--background-wait` (which returns once the data sources record; `--background` and a pause before Android 12);
SIGTERM to the recorded PID stops it, and the file is pulled once it stops growing (it is written just after the
process exits). The trace is kept in the run directory and summarized by Perfetto's `trace_processor_shell` with SQL:

- startup: `android_startups` (the app's last start), then its main-thread slices in that window;
- flows: FrameTimeline frames with `App Deadline Missed` and the main thread during them, then the busiest
  main-thread work over the whole run;
- both: GC count and time in the window.

Slices aggregate by their path of normalized names (numbers, `key=value` details and paths dropped), so repeated
work reads as one line, and each line follows the biggest part while it is at least half of its parent, eliding the
framework in between: `Choreographer#doFrame ×120 1674 ms → … → View#onTouchEvent ×94 1164 ms → slowScroll ×56
1120 ms`. The app's process is matched by name or, when the trace has it under `zygote64`, by its main thread's
name (the package's last 15 characters).

**Trace processor.** Pinned to Perfetto v58.2 (the SQL modules change between releases) with a SHA-256 per platform
(macOS and Linux, arm64 and x86-64). Found as `MDH_TRACE_PROCESSOR`, the cache (`~/Library/Caches/mdh`,
`$XDG_CACHE_HOME/mdh`), Perfetto's own download of the same build (`~/.local/share/perfetto/prebuilts`), or on
PATH. Never downloaded unasked: without it the trace is still kept and the finding tells the agent to ask its user;
`mdh perf setup` asks at a terminal, otherwise fails with `NEEDS_CONSENT` until called with `--yes` (MCP: `consent:
true`), then downloads with curl and checks the hash. `mdh perf explain <trace> --app <pkg>` summarizes a kept trace
afterwards.

**Later:** warm starts and time to full display (`reportFullyDrawn`), per-frame detail from `framestats`, frame
drops counted from FrameTimeline on every run, Java/native heap and graphics memory as metrics, perf findings in
the compatibility matrix.

## 12. Compatibility (`mdh-compat`)

Not a check kind (ADR-0009) and not a matrix by default (ADR-0011): a pipeline from the change to a verdict per
risk. The first two stages need no device.

```
mdh-impact                 mdh-compat::risk             mdh-compat::plan            mdh-compat::run
changed declarations  ──▶  rules × knowledge base  ──▶  fewest cells, cheapest ──▶  cells on devices,
+ compat facts             → risks (dimension,          first; cost and consent    flows + checks per risk
(syntax, ms)               reason, evidence, screens)                              → verdict per risk
```

**Facts from impact.** Per changed declaration (kept out of the serialized report): what it uses by name (calls,
types, constants, with their receivers), the API levels it compares `SDK_INT` with or requires
(`@RequiresApi`, `@TargetApi`), its resource qualifiers, the manifest element and attributes, the screens it
reaches. Per project: `minSdk`, `targetSdk` and `compileSdk` in the base and now (read from the Gradle scripts and
the version catalog as text; a value computed at configuration time is reported unknown), the resource qualifier
directories, the manifest's `uses-feature` entries.

**Knowledge base.** Its own repository, [compat-kb](https://github.com/mobile-dev-harness/compat-kb)
(ADR-0012); a pinned release is compiled into the binary (`crates/mdh-risk/kb/android.yaml`, version and
SHA-256 in `kb/SOURCE`, replaced by `scripts/update-kb.sh <version>`, checked by a test), and `MDH_COMPAT_KB`
points a binary at another copy. Each entry has a source link:

- behavior changes by API level and target SDK: the version, whether it applies by the device's version or the
  app's `targetSdk`, the names that trigger it, what to verify;
- form-factor triggers: qualifiers, APIs (window size classes, folding features, multi-window), manifest
  attributes;
- vendor quirks: the vendors, the names that trigger them, what goes wrong and how it shows.

**Risks.** `Risk { dimension, id, reason, evidence, screens, likelihood, verify }` where `verify` is a set of cell
requirements or `Unverifiable { needs }`. Risks with the same id and requirement merge across declarations.

**Cells.** A device requirement (API level, form factor, vendor) plus a configuration. Configurations are applied
to a running device and restored on every path (like visual variants): `wm size` / `wm density` computed from the
target size in dp and the panel (a tablet at 800×1280 dp on a 1344×2992 panel at 480 dpi becomes 1340×2144 at 268
dpi, so `sw600dp` resources and window size classes apply), `user_rotation` with auto-rotate off, font scale, night
mode, app locale. The planner covers every risk with the fewest cells, cheapest first; the current device in its
default configuration is always a cell, as the reference.

**Devices.** The current device; other local AVDs matched by API level (started with consent, at most two new
ones per run, stopped afterwards if mdh started them); physical devices matched by `ro.product.manufacturer`.
Missing ones make their risks unverifiable with the command that would add them. The `DeviceProvider` interface
(`candidates(spec)`, `acquire(offer)`) leaves room for AVD creation and cloud farms.

**Execution.** The APK is built once and installed per device (install-if-changed). Per cell: apply the
configuration, run the flows that pass the risk's screens with the functional and UI-rule checks, or open each
screen by its deep link and check it; device-type risks add a state check — the values of inputs and toggles
before and after a rotation (or a fold) must match. The report is per risk: verified, failed (cell, check,
evidence), unverified (needs); identical failures across cells are reported once.

**Judging (✅).** The reference cell runs every flow the device's other cells run. On each cell, after a flow
passes, the screen it ended on is checked with the tree rules that are never deliberate (`touch_target`,
`label`, `overlap`, `obscured`), keyed by rule and element; for screen-size and device-type risks a violation the
reference doesn't have fails the risk (`tablet 1280×800 dp: overlap — #email overlaps #sign_in`). A flow failing
only on a cell fails its risks, its crash first if it crashed (`Pixel_6_API_32 (API 32): login-wrong-password: no
crash: … NullPointerException`); failing on the reference too is reported as not a compatibility difference, and
failing on the reference cell itself as a functional failure to fix first. A flow that couldn't run (a missing
secret) says so; a cell where nothing relevant ran leaves its risks unverified. The
state check rotates to landscape and back and compares the elements with a unique id: input values, check states,
texts (`#selected: text "Opened Message 40" → "Tap a message"`), elements gone. A risk with no flow or deep link
through its screens is unverified ("save a flow that does"); risks from build settings (`targetSdk`, `minSdk`)
use every flow.

## 13. UI consistency checks (`mdh-visual`)

A check kind (ADR-0009): `Visual` implements `Check` and reports rule violations and baseline deviations as
findings, with a screenshot of the failing checkpoint (`visual-<checkpoint>.jpg` in the run directory) as
evidence. The entry points pass it to every verification (`VerifyOptions::checks`); flows run it at each
checkpoint — every `assert` step (`step-N`) and the end (`final`) — with the flow's `visual:` section as its
configuration, which the engine passes through uninterpreted.

**Inputs.** The compact tree (roles, labels, bounds, states), the screen density (`wm density`, override first)
to convert pixels to dp, later a full-resolution screenshot (not the downscaled agent JPEG).

**Rule checks (✅).** On the tree, for controls (buttons, items, inputs, toggles, sliders, tabs):

| Rule | Fails when |
|---|---|
| `touch_target` | narrower or lower than 48 dp; rows clipped by their scrolling container and controls cut by the screen edge are skipped, they can't be measured |
| `label` | no text or content description |
| `overlap` | two controls, neither containing the other, cover ≥ 30% of the smaller one |
| `obscured` | mostly under the status or navigation bar (not while the keyboard is up) |
| `duplicate_label` | two controls outside lists share a label; always a warning |

Without a `visual:` section every rule runs and violations are warnings, so verdicts report them without failing
on them; `rules: [...]` or `all` makes them fail, `none` turns them off. Content pushed off the screen can't be
found this way: accessibility bounds are clipped to the visible area, so a control cut by the edge looks like one
that ends there. Contrast is measured on pixels (below).

**Structural baselines (✅).** `.mdh/baselines/visual/<scope>/<checkpoint>/<profile>.tree.json`: the scope is the
flow (or the name given to `mdh visual check --baseline`), the profile the screen size and density
(`1344x2992-480dpi`), since layouts differ by device. A snapshot lists every element with an identity (role and
id, else role and label, plus an occurrence index), its label, detail and bounds in dp; `ignore` targets
(clocks, counters) are left out. Comparison reports elements added and missing, text and detail changes, and moves
or resizes beyond `tolerance_dp` (4). The first run records the baseline (a warning: commit it); a deviation fails
the check and writes `<profile>.tree.new.json`, which `mdh visual approve [scope]` promotes. A matching run removes a
stale candidate. Baselines are committed with the project (`.mdh/baselines/`).

**Pixel baselines (✅).** With `baseline` (unless `pixels: false`) each checkpoint also keeps
`<profile>.png`, the full-resolution screenshot halved (averaging away most anti-aliasing). Comparison runs in
8×8 blocks of the half frame (16 device px): a pixel differs when a channel moved by more than 32, a block changed
when at least 8% of its unmasked pixels (and 6 or more) differ, and 8-connected changed blocks form one region,
reported with its size and position in dp, the share of its pixels that changed and the smallest element
containing its center. Masks: system bars and keyboard (`ScreenInfo.obstructions`), `ignore` elements, focused
text fields (the cursor blinks) and `mask` rectangles in dp. Frames of another size fail as such. A failure writes
`visual-<checkpoint>-diff.png` (the frame dimmed, regions outlined) and the candidate `<profile>.new.png`, approved
with the tree. Same device profile only: GPUs and software rendering differ too much for cross-device pixels.

**Contrast (✅).** On the full-resolution screenshot, per labeled leaf element (texts, buttons, items, toggles,
tabs; disabled controls exempt): colors bucketed to 4 bits per channel (bucket means, so results don't depend on
sampling order), the most common bucket is the background, the most contrasting bucket covering at least 0.5% of
the samples is the text; WCAG ratio against 4.5:1, or 3:1 for text elements at least 32 dp tall. Thin or
multi-colored text is measured on its most visible color only, so contrast stays a warning unless a flow lists it.

**Cross-config layout checks (✅, one device).** `configs` (`font_scale`, `dark`, `rtl`, `all`) run at the `final`
checkpoint (or for `mdh visual check`), since switching recreates the app's activities and later steps wouldn't
expect that. Each variant reads the setting (`settings get system font_scale`, `cmd uimode night`, `cmd locale
get-app-locales`), sets it (1.3; dark; the app's language `ar`, right to left when the app supports RTL), waits for
the screen to settle again, and reports elements with an id missing compared with the default screen and rule
violations that only appear in that configuration (by rule and element, so known problems aren't repeated). The
setting is restored whatever happened; a failed restore is reported. A failing variant gets its own screenshot.
Truncation needs data accessibility doesn't expose; candidates are OCR of the element region compared with its
text, or the helper reading `Layout` ellipsis counts of `TextView`s (backlog).

**Design mocks (M8).** Figma's REST API provides rendered frames and node geometry, text and styles. Frames are mapped
to screens, elements matched by text or layer name, geometry compared after scaling by density, and deviations in
position, size, spacing, color and font size reported with a side-by-side diff.

## 14. MCP server (`mdh-mcp`)

- Built on `rmcp` 2.x over stdio and compiled into the `mdh` binary (`mdh mcp [--device <serial>]`). Tool input
  schemas are generated from Rust types with `schemars`, so docs and implementation can't drift.
- **One session per connection**, in memory; the device is connected on first use (`mdh_status` can switch
  devices or reset). Same session engine as the CLI, so behavior is identical.
- **Tools.** The default set (`--tools core`): `mdh_status`, `mdh_observe` (optional diff and screenshot), `mdh_act`
  (a list of actions, waiting for a target among them, stopping at the first failure; each reports what changed),
  `mdh_app`, `mdh_run`, `mdh_verify`, `mdh_flow`, `mdh_impact`; `mdh_observe` with `logs` returns the app's recent
  log lines. Actions are one flat object (an `action` and the fields it uses) rather than a `oneOf` per action. `--tools all` adds the check kinds `mdh_visual`, `mdh_perf`,
  `mdh_compat`; agents with a shell use their CLI commands instead, which cost no context until used (the plugin's
  `visual`, `perf` and `compat` skills describe them). Targets are strings in the same grammar as the CLI, so agents
  learn one syntax.
- **Results are the compact text the CLI prints**; screenshots are image content (JPEG, long edge 1024).
  `structuredContent` is not sent for now: clients commonly forward it to the model next to the text, which would
  double the tokens of every observation; machine-readable output is available through the CLI's `--json`.
- **Errors** are tool results with `isError` and `error[CODE]: message` plus a hint, so agents can branch on the
  stable code. A crash of the app sets `isError` while still returning the observation (the CLI's exit code 5).
- **Every input schema must have an object at its root** (the MCP spec; rmcp panics at startup otherwise), so
  parameters are structs — a tagged enum at the root would generate `oneOf`. A test asserts this for all tools.
- **Every definition is paid on every request.** Descriptions are one sentence (guidance lives in skills, loaded
  when needed), and the generated schemas are compacted when the server starts (`schema.rs`): integer formats and
  bounds, `null` alongside optional types and `$defs` references go. The benchmark showed what this costs: the
  previous 13 tools added ~7k tokens to every request, more than a setup with only adb. Now the core tools are
  5.9k characters in 8 tools (a test keeps them under 6.5k) and the instructions 0.6k.

## 15. Claude Code plugin

The repository doubles as a plugin marketplace (`/plugin marketplace add mobile-dev-harness/mobile-dev-harness`, then
`/plugin install mobile-dev-harness@mobile-dev-harness`):

```
.claude-plugin/marketplace.json  # at the repo root; one plugin, source ./integrations/claude-code
integrations/claude-code/
  .claude-plugin/plugin.json
  .mcp.json                      # { "mcpServers": { "mdh": { "command": "mdh", "args": ["mcp"] } } }
  skills/verify/SKILL.md         # impact → run → drive each affected screen → verify → flows; done = passing verdict
  skills/debug-crash/SKILL.md    # read the report, find the root cause, reproduce, fix, prove, save a flow
  hooks/hooks.json               # SessionStart → `mdh hook session-start`; Stop → `mdh hook stop`
```

The plugin carries no code of its own: hooks call `mdh hook <event>` (guarded with `command -v mdh`, so a
missing binary is silent), which reads the hook's JSON from stdin and stays silent outside Gradle projects.

- **SessionStart** returns `additionalContext`: that the project is verified with mdh, the devices online and the
  number of saved flows.
- **Stop** blocks once (never when `stop_hook_active` is set) when app files — sources, resources, manifests,
  build scripts, as `git` sees them, `.mdh/` excluded — were modified after both the session started (the
  transcript's creation time) and the newest passing `verdict.json` under `.mdh/runs/`. The reason lists the files
  and the protocol, and says to state explicitly what couldn't be verified. Work that was already uncommitted
  before the session doesn't trigger it.

`mdh init` gives other agents the same protocol as a section of `AGENTS.md` (between `<!-- mdh:begin -->` and
`<!-- mdh:end -->`, replaced in place on later runs) and sets up `.mdh/` with a `.gitignore` that keeps `flows/`.

## 16. Error model

- Every `mdh_core::Error` variant maps to a stable `code` (`DEVICE_NOT_FOUND`, `BUILD_FAILED`,
  `ELEMENT_NOT_FOUND`, `AMBIGUOUS_TARGET`, `APP_CRASHED`, `TIMEOUT`, `CAPABILITY_MISSING`, `CONFIG_INVALID`, …)
  and **must carry a hint** telling the agent what to do next.
- Mapping: error code → exit code → `isError` in MCP results.
- Rule of thumb: **prefer one more piece of useful context over making the agent guess** — e.g. timeouts include
  the last observation, missing elements include candidates.

## 17. Concurrency and process management

- tokio runtime. All external processes start through one `process` module that handles timeouts, kill-on-drop
  and output size limits, so processes like logcat can't become orphans or exhaust memory.
- Actions on one device are serialized (a lock per device); different devices can run in parallel (groundwork for
  parallel flow replay across devices).
- Logs are read incrementally on demand in the first release; if real-time crash alerts are needed later, each
  session gets a background logcat task.

## 18. Technical risks

| Risk | Impact | Mitigation |
|---|---|---|
| `uiautomator dump` is slow and unreliable | High observation latency, flakiness | Helper in M1 (ADR-0007); dump only as a fallback |
| No Chinese input | Form-based checks impossible for apps with Chinese users | Helper `set_text` (M1) |
| Incomplete semantics in Compose, RN, WebView | Content invisible in the tree | Opaque-region detection + annotated screenshot fallback; docs guide developers to add `testTag`/`contentDescription` |
| One UiAutomation client per device | Conflicts with mobile-mcp, Appium, Maestro on the same device | Documented; `session reset` stops the helper; clear error hint |
| AGP version differences | Probing fails | Version-matrix tests; allow manual override in config when probing fails |
| adb output differences across Android versions | Parse errors | Fixtures across API levels; tolerant parsers that ignore unknown fields |
| Emulator perf numbers are noisy and unrepresentative | False perf regressions, misleading budgets | Baselines per device, statistical + absolute thresholds, variance in reports; absolute budgets for physical devices |
| Compatibility matrices get expensive | Slow runs, CI cost | Cheap configuration axes on existing devices first; includes/excludes; reused AVD snapshots; bounded parallelism |
| Truncation and design deviations aren't visible in accessibility data | Missed UI issues | Pixel comparison and OCR candidates; optional debug-build hook; decided in M5 |
| Vendor coverage needs devices we don't have | Vendor bugs missed | Physical-device and cloud providers behind one interface; quirk knowledge base |
| Rust raises the contribution barrier | Slower community growth | Clear crate boundaries, good-first-issues (e.g. adding a diagnostic parser), thorough fixture tests |

## 19. Observability

- Logging via `tracing`, enabled with `-v` or `MDH_LOG=debug`; logs go to stderr so stdout JSON stays clean.
- Every result carries per-phase `timing_ms`; the benchmark reuses these numbers.
- No telemetry of any kind.

## 20. Testing

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

## 21. Release and distribution

- **Versioning:** semver, 0.x for now; JSON output carries a `schema` version and changes are called out in the
  CHANGELOG.
- **Binaries:** GitHub Releases built with `cargo-dist` on `v*` tags (macOS arm64/x64, Linux x64/arm64 ✅ since
  0.1.0; Windows x64 once mdh runs there), with shell
  and PowerShell installers.
- **Channels:** crates.io (`cargo install mobile-dev-harness`), a Homebrew tap; later an npm wrapper
  (`npx mobile-dev-harness mcp`, convenient in MCP configs).
- **Helper APK:** released with the same version as `mdh`, downloaded on demand and verified at runtime.
- **Claude Code plugin:** distributed through the marketplace in this repository.

## 22. Security

- Only the target app is touched by default. Changes to other packages or global settings are either on an
  allowlist (animations, locale, dark mode) and restored at session end, or require an explicit flag.
- Destructive operations on physical devices require `--allow-device-changes`.
- Secrets are only referenced through env variables and are redacted in output, recordings and run directories.
- The helper only listens on the device's loopback interface, reachable through adb forward, never the network.

## 23. Change impact analysis (`mdh-impact`)

ADR-0010; features F14. `mdh impact [--base REF]` / `mdh_impact` answer "what does this change reach, and what
should be verified?" from the source alone: no device, no build, code that doesn't compile is fine.

```
git diff --relative -M <base>, untracked files      ──▶ change set (paths relative to the Gradle root)
git ls-files (tracked + untracked, not ignored)     ──▶ every Kotlin/Java/XML/resource file, parsed on all cores
git cat-file --batch <base>:<path>                  ──▶ base versions of the changed files, parsed the same way
                ▼
declaration diff (by key)  ──▶  seeds  ──▶  users, breadth-first, per seed  ──▶  screens · reach · callers · verify
```

### 23.1 Extraction

One `FileIndex` per file: package, imports, **declarations** (types, functions, properties, constructors; Android
resources; manifest entries) and **references** (calls with argument counts, types, class literals, resource
references, other names, file names in string literals), each reference attributed to its innermost enclosing
declaration.

- **Kotlin** (`tree-sitter-kotlin-ng`) and **Java** (`tree-sitter-java`): receivers get a type where syntax tells:
  `Checkout().pay()`, `Log.e()`, properties and parameters with declared types or constructor initializers
  (`private val repo = Repo()`, `by viewModels<LoginViewModel>()`). Local functions, classes and properties are
  not declarations; what they use counts for the enclosing one. Overloads get keys with their parameter lists.
  `R.layout.main` and `binding` classes (`ActivityLoginBinding` → `activity_login`) become resource references.
  A class literal records the view id that triggers it: an `R.id` beside it (`R.id.open_login to
  LoginActivity::class.java`) or the view whose click listener contains it.
- **XML** (`tree-sitter-xml`): a layout or menu is a declaration, and so is each view with an `@+id` (its
  attributes are its body), with its label (`android:text`, …) kept for reach paths; values entries are
  declarations with their value; navigation graphs give destination edges; the manifest gives components,
  permissions, the launcher activity and deep links (`scheme://host/path`).
- **Signature vs. body.** A declaration's signature is the normalized token sequence of what users depend on
  (modifiers, parameters, return type, supertypes); its body is a hash of the rest. Tokens skip comments and
  whitespace, so a change that only touches comments or formatting changes neither, and the file is reported as
  cosmetic. A type's body leaves out its members, which are diffed on their own.
- **Parse errors.** tree-sitter recovers; a Kotlin file with errors is parsed again with soft-keyword calls
  (`open(…)`) masked, keeping the attempt with fewer errors. Remaining errors in a changed file are reported.

### 23.2 Resolution

By name, narrowed by what syntax knows, with a confidence on every edge:

| Reference | Candidates | Exact when |
|---|---|---|
| Resource (`R.string.x`, `@string/x`) | resources of that type and name, every qualifier | always |
| Type, class literal | types of that name | one candidate in the file, imported, or in the package |
| Call/name with a known receiver type `T` | members of `T`, `T.Companion`, extensions of `T`; members of `T`'s supertypes (likely) | one member of `T` |
| Call/name without a receiver | members of the enclosing types and their companions; members inherited from project supertypes; top-level declarations in the file, the package or imported | one candidate in the enclosing scope, or one visible top-level declaration |
| Call/name with an unknown receiver | members and extensions anywhere | never (`likely` for one, `ambiguous` for up to four, dropped above) |

Overloads of one function are one target: the argument count picks one when it can, otherwise the edge is
`likely`.

### 23.3 Propagation and screens

Each changed declaration (added, signature or body changed) is a seed, followed on its own so it keeps its own
path to each screen. From a declaration, the next ones are: the declarations containing its uses; subclasses of
a changed type or of the type declaring a changed member; for an override, the member it overrides (calls go
through the interface an injected dependency is typed as); for a manifest entry, its component class.
`ambiguous` edges are followed only from the seed itself; depth is capped at 12.

Screens are concrete classes whose project-visible supertypes end in `Activity` or `Fragment`, and composables
named `…Screen` or `…Route` (not previews). Reaching an activity or fragment, or one of its members, stops
there; a composable screen is recorded and followed further to find its host activity. Per screen the report
keeps the best path (confidence, then signature over body changes, then shortest) and how many changes reach it.

**Reach.** Screen-to-screen edges come from class literals of screens (intents) and fragment constructors, each
attributed to the screens above its use, and from navigation-graph actions; the trigger's view id is shown as
its label. Reach is the deep links of the screen (or of a composable's host), then the shortest tap path from the
launcher activity.

### 23.4 Findings

- **Before → after** for each modified declaration: calls, class literals, resource and literal references it
  gained or lost (`+ SettingsActivity::class · - finish()`).
- **Callers of changed signatures**, calls whose argument count no longer fits first; **dangling uses** of
  removed declarations (resources exactly; code by scope, import or receiver type).
- **What to verify**: functional — the affected screens; UI — changed resources (except ids), composables and
  custom views, with the screens each reaches; performance — list binding, drawing, lazy lists, `Application`
  and launcher startup; compatibility — manifest entries, qualified resources, API-level branches, default
  strings whose translations may now be stale or are missing; tests — test classes using reached code.
- **Limits**, always stated: reflection, dependency injection, generated code and run-time routes aren't
  followed; build-script changes are reported as affecting everything.

### 23.5 Cost

Measured on Now in Android (386 files analyzed, 1,900 declarations, 20,000 references) on an M-series laptop:
~60 ms to index on all cores, ~15 ms to analyze, ~140 ms end to end including git. The sample app takes ~100 ms,
mostly git. Text output is budgeted per section (12 changes, 10 screens, 4 call-site groups, …) with folded
counts; `--json` carries everything.

## 24. Verification engine (`mdh-verify`)

ADR-0009; features F6, F7. One entry point per question: `verify` (checks on the app as it is now) and
`run_flow` (replay, then checks). Both produce a `Verdict`.

### 24.1 Checks and verdicts

Check kinds implement `Check` (`kind()`, `run(&mut CheckContext) -> Vec<Finding>`); the context carries the
session, the run directory, the flow step and the verification window (device time since when crashes and logs
count). A `Finding` is one checked expectation: outcome (`pass`, `warn`, `fail`, `error`), the check in inline
syntax, what was observed when it didn't pass, the step, and evidence excerpts (a crash report). The verdict's
status is the worst outcome: `error` when a check couldn't be evaluated (an ambiguous target), else `fail` or
`pass`; `VERIFICATION_FAILED` (exit 1) carries the counts, the verdict the details.

Functional checks (`Functional`, built in):

- **Screen checks** (`visible`, `not visible`, states, `text`, `screen`) are evaluated on fresh observations,
  re-read every 250 ms until all hold or the timeout (3 s) passes, or the app crashes. Evaluating them once
  would turn "the result is still loading" into a false fail; waiting on a fixed sleep would be slower and still
  flaky. Targets resolve like actions do (refs, selectors, labels; a control wins over the text labeling it); an
  element that isn't there fails with the closest candidates, one that matches several elements is an `error`
  with them, and `visible` fails for an element covered by system windows.
- **`no crash`** is added to every verification. It reads the logs of the whole window (the session since it
  started watching, or the flow since it started), not just what is new: a crash the agent was shown after an
  action and then moved past still fails the verdict. That is the false pass this check exists for.
- **`log` / `no log`** search the app's log lines of the last 10 minutes.

### 24.2 Evidence

Each verification gets `.mdh/runs/<unix ms>-verify/` (flows: `-flow-<name>/`; the newest 20 run directories are
kept, builds included): `screenshot.jpg` (1024 px), `tree.txt` (the compact tree), `logs.txt` (the app's recent
info-level lines and crash reports) and `verdict.json`. Evidence is best effort; a screen that can't be captured
doesn't change the verdict. The verdict text names the directory but carries none of it, so evidence costs the
agent no tokens unless it opens a file.

### 24.3 Flows

`Flow` is the YAML of functional design §4.6. Saving converts the session's recorded actions: refs were already
replaced by selectors when they were recorded, typed passwords (recorded as `<secret>`) become
`${env:MDH_<FIELD>}` named after the field's id or label, and the names of those variables are reported.

Replay (`run_flow`):

1. Every `${env:…}` the flow uses must be set, or the run fails with `MISSING_SECRET` before touching the app.
2. Setup: system animations off (unless `setup.animations: true`; restored at the end, and only if the run
   turned them off), the `device` settings (each read first and put back at the end, last first; the screen is
   read before them so the helper is connected: a UiAutomation connection that disconnects puts back the
   rotation it found), `reset: data` (`pm clear`), `permissions` (`pm grant`), then a clean start: the app is
   stopped and launched, or stopped and opened through `setup.open` (`am start -a VIEW -d … <package>`). After
   the start the settings are read back and set again if the device dropped one; one that doesn't hold is an
   ERROR.
3. Steps run in order. A step aimed at an element first waits for it (`--step-timeout`, 10 s) to be on screen
   with part of it clear of the system windows, so a step right after the keyboard was dismissed doesn't find its
   target still covered; an element that never shows fails the step with what was on screen instead (closest
   elements, current activity), one that stays covered with that. An app crash
   during a step fails it. The first failing step stops the flow; the remaining steps and the final checks are
   skipped, `no crash` still runs, and the verdict says how many steps completed.
4. `assert` steps and the final `assert` run `Functional` with the step number.

**Which flows a change needs.** Saving records `screens`, the activities the steps ran on plus the one the
session ended on. `flows_for` picks the flows whose screens include a screen `mdh-impact` reports as affected or
the activity hosting it, or every flow when a build script changed; `mdh impact` lists them, and `run_changed`
(`mdh flow run --changed`, `mdh_verify` with `changed`) replays them. `mdh-verify` owns flows, so it fills
`verify.flows` into the impact report; `mdh-impact` stays independent of it.

`run_flows` replays several flows one after another; `junit()` renders their verdicts as one test suite with a
test case per flow, the first failed check as the failure message and the verdict text as its body.

**End to end in CI.** `.github/workflows/e2e.yml` builds `mdh` and the sample app, boots an API 34 emulator,
runs `mdh run` and replays the sample's flows (`examples/android-sample/.mdh/flows`) with `--junit`, keeping the
report and the run directories as artifacts.

## 25. Devices and emulators (`mdh-control::devices`, `mdh-driver`)

F1.2–F1.4. The driver reports devices with their AVD (`ro.boot.qemu.avd_name`, else the emulator console's
`avd name`) and API level, and the AVDs that can be started (`emulator -list-avds`; API level from `<avd>.ini`'s
`target` or the system image path in `config.ini`).

**Choosing** (`resolve`, pure over the driver): an explicit `--device` (serial or AVD name) wins; then the project's
default from `.mdh/device.json`; then the only online device; with several online, the only emulator among phones
(functional verification prefers an emulator: resettable, the same every run); otherwise someone has to choose.
With nothing online the startable AVDs are offered, the project's first, then the newest system image. The result
is `Ready`, `Start` (an AVD the user named), `Choose` or `Offline`.

**Asking** is left to the entry point through the `Ask` trait: the CLI asks at a terminal (stdin and stderr are
TTYs) with a numbered list or `Start <AVD>? [Y/n]`, end of input counting as no; the answer becomes the project's
default. Without a terminal — agents' shells, CI, MCP — nothing is started or guessed: `NO_DEVICE` lists the
startable AVDs and `AMBIGUOUS_DEVICE` the candidates, with a hint to ask the user and then use `mdh emulator start`
or `mdh devices use` (MCP: `mdh_status` with `start_emulator` or `device`). How the device was chosen, when it isn't
obvious, is said once at the top of the next observation (`Session::notify`).

**Starting** (`emulator -avd X -netdelay none -netspeed full`, `-no-window -no-audio -no-boot-anim` headless): the
process runs in its own process group with output in a temporary log, so it outlives mdh. The new emulator is the
online emulator serial that wasn't connected before and runs that AVD; it is ready when `sys.boot_completed` is 1
(timeout 240 s, after which the process is killed). If the process exits, the error carries its `PANIC`/`ERROR`
line. An AVD that already runs is used as is. A CLI session kept for the serial is discarded after a boot.
**Stopping** (`emu kill`) only applies to emulators and waits until adb no longer lists the serial.

