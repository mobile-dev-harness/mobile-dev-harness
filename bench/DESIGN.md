# Benchmark design, version 2

Status: draft, before the pilot. Version 1 (the ten tasks in `tasks/`, results in `results/`) is kept as it is.

**In short.** When a coding agent says an Android change works, can you believe it, and what does device access
change about that? Tasks are levelled by the evidence that decides them, from the diff (L0) to measuring on a
device under a condition (L3); how far a model gets without a device is measured per level. The app is Now in
Android at a pinned commit; every task has hidden checks that fail on the defect and pass on the fix, plus checks
that the rest of the app still works. An agent may answer that it couldn't verify. The headline is how often a
claim of success is false.

## 1. Why a second version

Every bug in version 1 is visible in the code. On 2026-10-04 (GLM-5.3, 119 runs) the agent alone, with no device,
was right in 28 of 28 runs; with mdh in 27 of 27 once a grader error is corrected (the fix was the reference
fix, the grader couldn't read the screen); with adb and mobile-mcp it lost runs only to timeouts. The agent alone answered FIXED in all 12 fix runs although the prompt asked it to check
the fix and it had no way to; it was right because the bugs were in plain sight. A benchmark where reading is
enough can't show what checking is worth, and one that scores an unchecked "fixed" as correct rewards the guess
that verification exists to catch.

## 2. Related work and this benchmark's place

| | Tasks | Oracle | Measures |
|---|---|---|---|
| SWE-bench (2310.06770), Multimodal (2410.03859) | real issues and PRs; Python, JS with images | the PR's unit tests | resolved rate |
| SWE-Bench Mobile (2602.09540) | PRD + Figma features in a production iOS app | test suites, hosted | task success (≤ 12%) |
| MobileDev-Bench (2603.24946) | 407 real issues, 19 apps (Android, React Native, Flutter) | test patches, containers | resolved rate (3–6%) |
| AppEval (2608.18588) | 200 Android repairs from 24 repos (+ iOS, HarmonyOS) | instrumentation test on the installed app; infrastructure failures a separate outcome | Pass@1 (22–90%) |

They ask whether the agent's patch resolves the task. None asks whether the agent's own "fixed" or "works" is true,
has review tasks (judge a teammate's change), or compares an agent's tooling on the same model. This benchmark
does those three. Taken from them: real issues with the reference fix kept apart from the hidden checks;
FAIL_TO_PASS and PASS_TO_PASS checks (SWE-bench); acceptance only when the check fails on the defect and passes on
the fix on the same installed app, with infrastructure failures as their own outcome (AppEval); a human review of
every task (SWE-bench Verified). Name: `mobile-agent-bench` ("SWE-Bench Mobile" is taken). Now in Android is not
among MobileDev-Bench's apps; AppEval's list is not public yet.

## 3. Tasks

**Fix**: an issue against the code; the agent fixes it. **Verify**: a teammate's uncommitted change and its
description; the agent judges it without changing code. Verify tasks come in pairs, one description with a
correct and a broken change, so a constant answer scores 50%.

The agent ends with one line: `RESULT: FIXED | NOT FIXED | UNVERIFIED` or `VERDICT: PASS | FAIL | UNVERIFIED`, and
a sentence why. UNVERIFIED means "I couldn't establish it": allowed, never counted as correct (section 6).

Issues are written as their reporter would: a user describes symptoms and mentions a condition only if they would
know it ("in dark mode", "on my tablet"), never the cause. Change descriptions are written as a PR description.

### 3.1 Levels

A task's level is the least evidence that decides it:

| Level | Decided by | Kinds of bug |
|---|---|---|
| L0 | the code or the diff | an inverted condition, a wrong id, a misspelled constant |
| L1 | running the app on one path | data the code mishandles, a runtime-only crash, a library or framework default, state that doesn't update |
| L2 | running under a condition, or over several steps | dark mode, font scale, time zone, locale, rotation, process death, window size, keyboard, back stack, permissions |
| L3 | measuring, or looking away from the change | jank, startup time, a few pixels of overlap or clipping, behavior on another API level, a shared component breaking another screen |

### 3.2 Calibration

A level says where a task's defect shows: in the diff, on one path, under a condition, in a measurement. Whether a
model gets there without that evidence is a difference between models, and a result, not a property of the task.
Each task is run three times by the agent alone with the strongest model available, and once with mdh to show it
is solvable:

- **Verify pairs** are scored per pair: a run decides the pair if it judges both changes right (a coin decides it
  25% of the time).
- **Fix tasks** are resolved when the hidden checks pass.
- How often the agent alone decides a pair or resolves a fix is reported per level and per model. A model that is
  right above L0 without a device inferred the behavior from the code, or ran the code off the device (the build,
  JVM tests, Robolectric): the task keeps its level.
- A task is repaired only when it is broken: nothing decides it, a check fails a correct answer, the prompt or the
  workspace gives the answer away. Calibration runs are kept and published but not scored.

### 3.3 Sources

1. **Real bugs, re-injected.** Now in Android's tracker has 179 closed bugs, 74 with a linked PR, of which about 20
   show at runtime. The bug is put back into the pinned commit and the issue's text kept: #1864 snackbar under the
   keyboard, #1295 no snackbar on wide screens, #1222 a blank query saved as a recent search, #611 the settings
   dialog lost on rotation, #541 the bookmark icon ignores the theme, #415 scroll position lost between tabs,
   #232 dark system bar icons in dark mode. Models may have seen these fixes: reported apart.
2. **Synthetic bugs** from the kinds in 3.1, with issues written for them; no model has seen them.
3. **Verify pairs**, from real PRs or written.

At most a third of the tasks come from source 1.

### 3.4 Authoring

Each task goes through: write (issue or change, defect, reference fix, checks) → `mdh-bench validate` (the checks
fail on the defect or broken change and pass on the fix or correct change) → a second, different correct fix
passes too, where one exists → calibration (3.2) → review → frozen with the set.

**Review** is done by a model from another family than the one that wrote the task (tasks are written with
Claude; reviewed with GLM-5.3 through `mdh-bench review`). The reviewer gets the task, its patches and checks,
and a checklist: the issue says what a reporter would know and not the cause; the defect matches the issue;
the reference fix is not the only reasonable one and the alternatives pass; the checks test behavior, not the
implementation; the level's reasoning holds; nothing in the workspace gives the answer away. It answers per item
with a reason; every objection is resolved, by a change or a written reply, before the task is frozen. Reviews
are kept with the task and published with it.

### 3.5 Format

`bench/tasks/<id>/task.yaml`, with the hidden checks in `checks/` and the review in `review.md`:

```yaml
app: nowinandroid                  # an entry in bench/apps.yaml; default: sample
level: L1
source: real                       # real (with upstream), synthetic, or pr
upstream: android/nowinandroid#1222
kind: fix                          # fix, or verify with `truth: pass | fail` and `pair: <the other task>`
summary: a blank query is saved as a recent search
prompt: |
  (the issue, as its reporter wrote it)
bug:                               # committed: the starting point (verify tasks: `change`, left uncommitted)
- path: feature/search/impl/src/main/kotlin/.../SearchViewModel.kt
  find: "        if (query.isBlank()) return\n"
  replace: ""
fix:                               # the reference fix
- path: ...
  find: ...
  replace: ...
alternatives: []                   # other correct fixes, as lists of edits; the checks must pass them too
probes: []                         # device observations after a check, for what a flow can't assert (section 5)
private: false
```

Edits are literal find/replace pairs; each `find` must occur once. `bench/apps.yaml` names each app's source (a
directory here, or a repository at a pinned commit), module, variant, package and build command; its
regression flows (PASS_TO_PASS) live in `bench/apps/<app>/regression/`.

## 4. Apps and environment

**Now in Android** (Apache 2.0, pinned commit, `demoDebug`: local static data, no backend). 37 Gradle modules,
17.5k lines of non-test Kotlin, Compose, Hilt, Room, DataStore, minSdk 23, targetSdk 36, adaptive layouts,
dynamic color. Too large to read whole. Its demo data has 19 topics and 311 articles, all published at 23:00 UTC,
2 of them without a type. **The sample app** keeps version 1's tasks. Later one or two more apps (Wikipedia,
Thunderbird; licenses to check; MobileDev-Bench uses Thunderbird, so no tasks from its issues there).

Pinned and recorded with every result: the app commit, JDK 17, the Gradle distribution, the agent (Claude Code
version), the system image and AVD (Pixel 9 Pro XL, API 36, 4 GB RAM), and the device state every run starts from:
time zone America/Los_Angeles, locale en-US, font scale 1, light mode, portrait, animations on, keyboard
enabled. The device identity is checked before and after every run. L3 tasks about API levels use a second
emulator (API 32); they are the only ones that start one, and the agent is told it exists.

Builds: a fresh copy of Now in Android builds in 84 s with warm Gradle caches (27.6 min the first time), 17 s
after an app-only change. A session builds the pinned commit once to warm the shared build cache, then builds
offline. Article images still come from the network; no check depends on them.

## 5. Grading

- **FAIL_TO_PASS**: hidden mdh flows that fail on the defect (or the broken change) and pass on the fix (or the
  correct change).
- **PASS_TO_PASS**: the app's regression flows (start, the three tabs, a topic, search, settings, bookmarking)
  must keep passing; a fix that breaks one isn't resolved.
- **Conditions**: the checks set the task's condition with `setup.device` (dark theme, font scale, time zone, the
  app's language, orientation, display size), which mdh puts back afterwards and verifies held while the app ran.
- **Probes**: what a flow can't assert is read from the device right after the check that sets it up, while the
  app is as that flow left it: an `adb shell` command and the text its output must or must not contain (the status
  bar's appearance, from `dumpsys window`). A probe is read again for a few seconds before it counts as failed.
- Checks test behavior, never the implementation, and accept every alternative fix.
- **Infrastructure errors aren't verdicts.** A check that ends in ERROR (the screen unreadable, the device gone) is
  rerun after a device reset; a second ERROR records the run as `grader_error`, left out of every rate and
  listed. (Version 1 counted two such runs as false passes.)

## 6. Outcomes and metrics

| Answer | It works | It doesn't |
|---|---|---|
| PASS / FIXED | correct | **false pass** |
| FAIL / NOT FIXED | false fail | correct |
| UNVERIFIED | abstained | abstained |
| none (timeout, no line) | no answer | no answer |

"It works" is the truth for verify tasks and the hidden checks for fix tasks. Per setup, per level and per
source:

- **False-pass rate**, the headline: of the runs where it doesn't work, the share answered PASS / FIXED. (As in
  version 1; it doesn't depend on how many tasks are broken.)
- **Claim precision**: of the PASS / FIXED answers, the share that are true; what a reader of the claim gets.
- **False-fail rate**: of the runs where it works, the share answered FAIL / NOT FIXED.
- **Correct**, **abstained**, **no answer**: shares of all runs. An agent that always abstains has no false
  passes and nothing correct; both numbers are shown together.
- **Resolved** (fix tasks): the hidden checks pass, whatever was said.
- **Unchecked claims**: PASS / FIXED answers from runs that didn't install the app after their last change to
  the code (from the transcript: `adb install`, `gradlew install…`, `mdh run`, mdh's `run` or `app install`
  tools, mobile-mcp's install). The device is reset before every run, so the app on it is the agent's version
  only once it installed one; relaunching after an edit runs the old one. Edits outside the workspace (the
  agent's test scripts) don't count.
- **Cost**: tokens, tool calls, screenshots, time; medians per run.

## 7. Size, budget, statistics

- **Pilot**: 21 tasks on Now in Android, five per level (two verify pairs and one fix task each) and a second fix
  task at L0. Calibration (63 runs, agent alone) and one mdh run each (21): 84 runs, about 10 hours. It answers
  whether the levels hold.
- **Full**: 100 tasks, 25 per level (eight verify pairs and nine fix tasks each), across apps.
  - Main model: four setups, one run each (400), plus two more runs of a fixed 20-task subset to measure
    run-to-run variance (160): 560 runs, 55–75 hours at 6–8 minutes a run.
  - Other models: the agent alone and mdh only: 200 runs.
  - Timeout 15 minutes (version 1: 25).
- Paired comparisons (the same tasks in every setup): about 25 points are detectable within a level, about 12
  over all 100 tasks.

## 8. Fairness, contamination, validity

- Bug kinds come from real trackers, not from what mdh can check. Every task is decidable with adb, screenshots
  and logcat alone. Every setup gets the same prompt apart from one paragraph about its tools.
- The task set is frozen before a published run; tasks, checks, prompts and transcripts are published. 20% of the
  tasks stay private so later models can't have trained on them; only their results are published, and they are
  run by the maintainers. Reviewing a private task sends it to the reviewer's provider, which is also evaluated:
  private tasks are reviewed last, after their content is final, and the provider is named in the review.
- Every workspace is a fresh repository without the app's history.
- Limits: one agent (Claude Code), one device model, mostly one app until more are added. The author of the
  tasks also builds mdh and the reviewer is a model under evaluation; the review checklist, the adb-only rule and
  publishing everything are the mitigation.

## 9. Pilot tasks

A task's level follows from where its defect shows (3.1). Every task is in `bench/tasks/nia-*`; `mdh-bench list`
shows them.

| # | Level | Kind | Task | Bug or change | What decides it |
|---|---|---|---|---|---|
| 1–2 | L0 | verify pair | `nia-settings-dark-order` | The dark mode options reordered; the broken change's Dark row sets Light | the diff |
| 3–4 | L0 | verify pair | `nia-topic-follow-button` | The topic screen's follow chip becomes a Follow / Following button; the broken change passes the current state, so tapping changes nothing | the diff |
| 5 | L0 | fix | `nia-topic-bookmark` | Bookmarking on a topic screen passes the topic's id: another article is saved | the view model |
| 10 | L0 | fix, real #1222 | `nia-search-blank-recent` | A blank query is saved as a recent search (upstream's fix and its tests taken out) | the view model: one `isBlank` |
| 6–7 | L1 | verify pair | `nia-search-empty-message` | "explorer" → "explore" in the no-results message; the broken change leaves the spaces between the sentence's parts to the string resources, where the build trims them: "exploreIntereststo browse topics" | one search without results; from the code, only by knowing that the build trims the spaces at the ends of a string resource |
| 8–9 | L1 | verify pair | `nia-search-recent-trim` | Recent searches saved without the spaces around them; the broken change trims the query as it is typed, so a space at the end never stays and words run together | typing two words a key at a time; setting the field's text at once hides it |
| 21 | L1 | fix | `nia-search-crash` | Topic rows and article cards in the search results share one key space: with a topic and an article of the same id on screen together ("compose" or "studio", scrolled a little) the grid crashes | the stack trace (`Key "2" was already used`); without a device, a guess at why some words crash |
| 11–12 | L2 | verify pair | `nia-card-date-format` | Card dates formatted with kotlinx-datetime and remembered; the broken change calls `Instant.format` without an offset, which is UTC | a time zone east of UTC+1 (the same dates in Los Angeles) |
| 13–14 | L2 | verify pair | `nia-settings-links-pinned` | The settings dialog's links stay below the scrolling options; the broken change gives the options no weight, so they take all the height when they don't fit | landscape: the links are gone (portrait is fine) |
| 15 | L2 | fix | `nia-tab-from-topic` | Tapping the tab you are in no longer leaves a topic opened from it (the report is #1614's; the cause is rebuilt in the Navigation 3 code) | three steps: a card's topic tag, then the tab |
| 16–17 | L3 | verify pair | `nia-app-bar-inset` | A refactoring of NiaApp's top inset workaround; the broken change consumes the top inset everywhere, and the topic screen, which has no app bar, slides under the status bar | a screen the change doesn't name, and where its controls sit |
| 18–19 | L3 | verify pair | `nia-search-link-wording` | The link in the no-results message reads "the Interests tab"; the broken change rewords the string the navigation bar and the app bar use too | the tab's label, away from the search screen |
| 20 | L3 | fix | `nia-system-bars-dark` | The system bars are styled once from the system's theme: with the app set to Dark on a light system the status bar icons stay dark (the report is #232's; the cause is rebuilt on `enableEdgeToEdge`) | the status bar's icons, with the app's own dark setting |

Changed from the first plan, and why:

- **Reading decided too much.** A removed guard or an explicit `UTC` in a diff is seen by a careful reader, as
  version 1 showed. The L1–L3 pairs now turn on what the code doesn't say: a rule of the build (6–7), how a text
  field behaves while typing (8–9), a library's default argument (11–12), a layout that only overflows when the
  window is short (13–14), a screen and a string away from the diff (16–19). How often a model still gets there
  without a device is what calibration measures (3.2).
- **6–7** was the cards' "date • type" line losing its blank-type check (a removed `if`).
- **10** was the L1 fix task. Its defect is a missing guard in plain sight in the view model, so it is an L0 task,
  and L1's fix task is **21**: a crash whose cause is in the log and not in the report.
- **13–14** was the settings dialog kept open across rotation (#611): `remember` against `rememberSaveable` reads
  from the diff.
- **15** was #1864, the snackbar's position with the keyboard open: it needs the device offline, which a flow can't
  set, and a check of where the snackbar sits, which a flow can't assert; upstream's fix also leaves the snackbar
  under the keyboard.
- **18–19** was jank while scrolling. On the pilot's emulator the unmodified app already drops every frame
  (`mdh perf flow` on the For you feed: 99% janky frames, p90 150 ms, CPU 100%), so frame timing can't tell a
  change apart; cold start (1.9 s ± 0.1 s) can, but parsing all 311 articles on the main thread adds only 40 ms.
  A performance pair waits for a device whose frame times leave room to get worse: a physical device, or an
  emulator on which the unmodified app scrolls smoothly. It isn't the load on the host: freshly booted, with the
  host's memory free, this headless emulator gave 100% janky frames again while the app used a third of one
  core.
- **20** keeps #232's report; its cause (a style name in `values-night`) no longer has an effect, because
  `enableEdgeToEdge` sets the bars at run time. **15** has #1614's report the same way: its cause was in the
  Navigation Compose code the app has since replaced. Both are `synthetic` with the issue in `upstream`: no
  upstream fix applies to them.

## 10. Work before the pilot

- mdh-bench: done. Apps (`bench/apps.yaml`), task levels, sources and pairs, the time zone reset before every
  run, grader errors retried once and left out, the three-way answers, unchecked claims, `regrade` (also
  re-reads transcripts) and `review`, per-level and per-source tables.
- mdh: projects with Isolated Projects on (Now in Android) couldn't be read; fixed.
- mdh: done. A step that can't be checked makes the verdict ERROR, not FAIL; flows set the device they need
  (`setup.device`); rotation locks in one step and survives the helper reconnecting.
- The device: 4 GB RAM for the AVD.
- mdh-bench: probes (section 5); agents may set the display settings a check needs (dark theme, font scale, time
  zone, rotation, display size), which the reset before every run puts back. Version 1's prompt forbade every
  device setting, which left L2 tasks undecidable.
- The tasks: 21 pilot tasks with checks, written and `validate`d. Still to do: the rest of calibration (3.2) and
  review (3.4); where the pilot stands and what comes next is in [STATUS.md](STATUS.md).

## 11. Open questions

- Version 1's results keep version 1's prompt (no UNVERIFIED); they aren't comparable to version 2's.
- The second app, and when.
- A performance task needs a device whose frame times mean something (section 9, 18–19).
- Typing: mdh sets a field's text at once, a keyboard a key at a time; tasks 8–9 are only seen the second way. Their
  hidden check types in two steps; an agent using `mdh_act` to type the whole query sees nothing wrong.
