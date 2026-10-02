# 02 · Technical Architecture

> Status: draft v0.3 · Companion docs: [01-functional.md](01-functional.md), [ADRs](../adr/)

## 1. Overview

```
            agent ──▶ mdh-mcp            mdh-cli ◀── humans / CI / agents (shell)
                         └───────┬───────────┘
   ┌──────────────┬──────────────┼──────────────┬──────────────┐
   │  mdh-verify  │   mdh-perf   │  mdh-compat  │  mdh-visual  │   quality domains (ADR-0008)
   │  assertions  │   startup    │  device and  │  baselines   │
   │  verdicts    │   frames     │  config      │  design      │   compat orchestrates the
   │  flows       │   memory/CPU │  matrices    │  mocks/rules │   other three across a matrix
   └──────┬───────┴──────┬───────┴──────┬───────┴──────┬───────┘
          └──────────────┴──────┬───────┴──────────────┘
                         ┌──────▼───────┐
                         │ mdh-control  │   sessions · targeting · actions · waiting · state · navigation
                         └──┬────────┬──┘
               ┌────────────▼─┐    ┌─▼────────────┐
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

Principles:

1. **Entry points are thin shells.** CLI and MCP only parse input and render output; all logic lives in the
   library crates, so both behave identically.
2. **Domains on a shared foundation.** Each quality domain owns its logic; observation, project handling and device
   access are shared. Dependencies only point downward.
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
| `mdh-project` | foundation | `ProjectAdapter`; Gradle probing, building, diagnostics; later RN, Expo, Flutter, Xcode | M2 |
| `mdh-control` | domain | Device selection, observation, input, app lifecycle ✅; session engine (refs across calls, targeting, waiting, diffs), state setup, navigation | M1, M3 |
| `mdh-verify` | domain | Assertions, verdicts, evidence, flow record/replay, JUnit | M4 |
| `mdh-visual` | domain | Baselines, structural and pixel diffs, cross-config layout checks, rule checks, design mocks | M5, M8 |
| `mdh-perf` | domain | Startup, frames, memory, CPU, budgets, baselines; later traces | M6 |
| `mdh-compat` | domain | Matrices, device pool and providers, config application, scheduling, matrix reports | M7, M8 |
| `mdh-mcp` | entry | MCP server (`rmcp`, stdio), compiled into the `mdh` binary | M1 |
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

Uses the same executor as 4.2, except that targets are always selectors, each step first waits for its target
(with a timeout), assertions are evaluated by `mdh-verify`, and a verdict with evidence is produced at the end.
Recording and replay share one executor so that whatever was recorded can be replayed.

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
the tests also record estimated token counts before and after compression to catch regressions.

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
- **Launches that don't navigate:** when the target app already has a task, `am start` may only bring it to the
  front and print `Warning: Activity not started, its current task has been brought to the front`. Launch and
  deep-link navigation must detect this and, depending on the reset policy, retry with `-S` (stop first) or
  report that navigation did not happen instead of claiming success.

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
debug, or `--variant`. Several candidates are an `AMBIGUOUS_BUILD_TARGET` listing them (exit 2).

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
  carry line numbers. The YAML crate is chosen in M3 (`serde_yaml` is unmaintained, so a well-maintained
  alternative is needed) and wrapped in an internal module so it can be swapped later.
- `.mdh/session.json`: CLI mode only, versioned; incompatible versions are discarded and rebuilt.
- `.mdh/runs/`: one directory per run; the latest 20 are kept (configurable).
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
- Input is injected **asynchronously** through `UiAutomation.injectInputEvent`. Synchronous injection waits until
  the target window has handled the event and was observed to take 0.4–1.7 s while the app animates; settling is
  the host's job (§6).
- `Helper::ensure` (host): reuse the forward → ping → on a missing or stale helper, check the installed
  `versionCode`, (re)install the APK embedded in the binary, start it, poll until it answers. An APK signed with
  another key is uninstalled first.
- Releases: `scripts/build-helper.sh` rebuilds the APK into `crates/mdh-driver/assets/`. The version code lives in
  three places (Gradle, `Commands.VERSION_CODE`, `HELPER_VERSION_CODE`) and must be bumped together.

**Measured:** warm `mdh observe` ~23 ms end to end; cold start ~360 ms; restart after a kill ~300 ms; tree ~7 ms
and tap ~36 ms (median) per helper request vs. ~120 ms for `adb shell input`. The foreground activity comes from
`dumpsys window displays` (~37 ms), fetched concurrently with the tree.

**Coexistence:** only one UiAutomation client can run per device. While the helper runs, other clients'
`uiautomator dump` is killed, so mobile-mcp, Appium or Maestro on the same device conflict with it. If the helper
can't start, mdh falls back to `uiautomator dump`.

**Later (M5):** window-change event stream (push instead of poll), screenshots through `UiAutomation`.

## 11. Performance (`mdh-perf`)

**Data sources** (all via adb, parsed by pure functions with fixtures):

| Metric | Source |
|---|---|
| Startup | `am start -W` (`TotalTime`, `WaitTime`, `LaunchState`); `ActivityTaskManager: Displayed` and `Fully drawn` in logcat |
| Frames | `dumpsys gfxinfo <pkg> reset` before, `dumpsys gfxinfo <pkg>` after (janky frames, percentiles, slow/frozen counts); `framestats` for per-frame detail |
| Memory | `dumpsys meminfo <pkg>` (TOTAL PSS, Java heap, native heap, graphics) |
| CPU | `/proc/<pid>/stat` sampled during the flow |

**Measurement protocol.** Warm-up runs are discarded; N measured runs report median, p90 and median absolute
deviation. Perf runs restore the device's real animation scales — control disables animations for stability, but
jank can't be measured without them. Runs are separated by a cool-down; device, API level and build fingerprint are
recorded with every result.

**Baselines and regressions.** Baselines live in `.mdh/baselines/perf/<device-fingerprint>/<scenario>.json`. A
regression requires both a statistical signal (new median beyond baseline median + k·MAD) and a minimum absolute
delta, so noise on emulators doesn't produce failures. Budgets in `mdh.yaml` are absolute and meant for physical
devices. Perf checks are also exposed as assertion kinds for `mdh-verify`.

**Later:** Perfetto traces around slow steps, summarized with trace processor to the slices that explain them.

## 12. Compatibility (`mdh-compat`)

**Matrix model.** Axes from `mdh.yaml` expand into cells; a cell is a *device spec* (API level, form factor, vendor)
plus a *configuration* (locale, font scale, night mode, density, size, orientation). Includes and excludes keep the
matrix small; pairwise reduction can come later.

**Device pool.**

```rust
#[async_trait]
pub trait DeviceProvider: Send + Sync {
    async fn candidates(&self, spec: &DeviceSpec) -> Result<Vec<DeviceOffer>>;
    async fn acquire(&self, offer: &DeviceOffer) -> Result<DeviceLease>;   // booted, ready, exclusive
}
```

Providers: local emulators (install the system image with `sdkmanager`, create an AVD from a hardware profile such
as a phone, tablet or foldable with `avdmanager`, boot headless, keep a clean snapshot, reuse across runs), physical
devices (matched by `getprop`: manufacturer, model, API level), and later cloud farms. Parallelism is bounded by host
RAM and CPU.

**Configuration without new AVDs.** A per-cell guard applies configuration on an existing device and restores it on
drop: `settings put system font_scale`, `cmd uimode night yes|no`, `wm density` / `wm size` (and `reset`), rotation
via `user_rotation`, and locale (per-app locales via `cmd locale` on API 33+; how to switch the system locale without
root on older images is to be verified in M7).

**Execution and report.** A scheduler runs (cell, flow) jobs on leased devices, each with its own control session,
and collects verdicts plus the selected perf and visual results per cell. The report is a matrix with evidence;
failures are clustered by signature (step, assertion, error code) so one bug isn't reported nine times.

**Vendors.** Vendor ROM differences (background restrictions, autostart, permission dialogs) only show on physical
devices. Leases record manufacturer and ROM version, and a small knowledge base of known quirks turns them into hints.

## 13. UI consistency (`mdh-visual`)

**Inputs.** The compact tree (roles, labels, bounds, stable keys), a full-resolution screenshot (not the downscaled
agent JPEG) and the screen density (`wm density`) to convert pixels to dp.

**Baselines.** `.mdh/baselines/visual/<screen-key>/<config-key>.{png,tree.json}`, where the screen key is a flow step
or activity plus route and the config key identifies device profile and configuration. Candidates are produced by
every check; `visual baseline approve` promotes them. Baselines are never updated automatically.

**Comparison.** Structural first: match elements by stable key and report added, missing, moved or resized beyond a
dp tolerance, and text changes — cheap, and it says what changed. Then pixels: mask the status and navigation bars,
regions given by selectors and explicit rectangles; compare with a per-pixel tolerance plus block-wise SSIM; emit a
diff image and map changed regions back to the elements that cover them.

**Cross-config layout checks.** Overlap (bounds of two text or interactive leaves intersect, excluding ancestors),
clipping (element partially outside the screen or its container), and elements missing compared with the default
configuration. Truncation needs data accessibility doesn't expose; candidates are OCR of the element region compared
with its text, or an optional hook in debug builds — decided in M5.

**Rule checks.** Touch targets ≥ 48 dp for interactive elements; interactive elements without a label; text
contrast from foreground and background colors sampled in the element's region (WCAG 4.5:1, 3:1 for large text);
duplicate labels. Google's Accessibility Test Framework could run in an optional second helper APK later; the base
helper stays dependency-free.

**Design mocks (M8).** Figma's REST API provides rendered frames and node geometry, text and styles. Frames are mapped
to screens, elements matched by text or layer name, geometry compared after scaling by density, and deviations in
position, size, spacing, color and font size reported with a side-by-side diff.

## 14. MCP server (`mdh-mcp`)

- Built on `rmcp` 2.x over stdio and compiled into the `mdh` binary (`mdh mcp [--device <serial>]`). Tool input
  schemas are generated from Rust types with `schemars`, so docs and implementation can't drift.
- **One session per connection**, in memory; the device is connected on first use (`mdh_status` can switch
  devices or reset). Same session engine as the CLI, so behavior is identical.
- **Tools (M1):** `mdh_status`, `mdh_observe` (optional diff and screenshot), `mdh_act` (a list of actions, stopping
  at the first failure; each reports what changed), `mdh_wait`, `mdh_logs`, `mdh_app` (launch, stop, install).
  Later milestones add `mdh_run`, `mdh_navigate`, `mdh_state`, `mdh_verify`, `mdh_flow` and the perf, compat and
  visual tools. Targets are strings in the same grammar as the CLI, so agents learn one syntax.
- **Results are the compact text the CLI prints**; screenshots are image content (JPEG, long edge 1024).
  `structuredContent` is not sent for now: clients commonly forward it to the model next to the text, which would
  double the tokens of every observation; machine-readable output is available through the CLI's `--json`.
- **Errors** are tool results with `isError` and `error[CODE]: message` plus a hint, so agents can branch on the
  stable code. A crash of the app sets `isError` while still returning the observation (the CLI's exit code 5).
- **Every input schema must have an object at its root** (the MCP spec; rmcp panics at startup otherwise), so
  parameters are structs — a tagged enum at the root would generate `oneOf`. A test asserts this for all tools.
- **Measured:** tool definitions ~1,400 tokens; server instructions 645 characters.

## 15. Claude Code plugin

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
- **Binaries:** GitHub Releases built with `cargo-dist` (macOS arm64/x64, Linux x64/arm64, Windows x64), with shell
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
