# Benchmark design, version 2

Status: draft, before the pilot. Version 1 (the ten tasks in `tasks/`, results in `results/`) is kept as it is.

**In short.** When a coding agent says an Android change works, can you believe it, and what does device access
change about that? Tasks are levelled by the evidence that decides them, from the diff (L0) to measuring on a
device under a condition (L3); the levels are checked by running the agent without a device. The app is Now in
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

A level is a hypothesis until measured. Each task is run three times by the agent alone with the strongest model
available, and once with mdh to show it is solvable:

- **Verify pairs** are scored per pair: a run decides the pair if it judges both changes right (a coin decides it
  25% of the time). L0: decided in at least 2 of 3 runs. L1–L3: in at most 1 of 3.
- **Fix tasks**: L0: resolved (hidden checks pass) in at least 2 of 3 runs. L1–L3: in at most 1 of 3, so the issue
  must invite a fix that reads right but doesn't work: a second cause, a condition, a decoy.
- A task that misses its level is moved or rewritten. Calibration runs are kept and published but not scored.

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
passes too, where one exists → calibration (3.2) → review by a second person (the issue is fair, the fix is not
the only possible one, the checks test behavior) → frozen with the set.

### 3.5 Format

```yaml
id: nia-search-blank-recent        # unique, stable
app: nowinandroid                  # an entry in bench/apps.yaml (repository, commit, variant, package)
level: L1
source: real                       # real (with `upstream`), synthetic, or pr
upstream: android/nowinandroid#1222
kind: fix                          # fix, or verify with `truth: pass | fail` and `pair: <id>`
condition: null                    # what the device needs, e.g. {timezone: Asia/Tokyo}; for the grader only
summary: A blank query is saved as a recent search
prompt: |
  (the issue, as its reporter wrote it)
defect: defect.patch               # applied and committed: the starting point
fix: fix.patch                     # the reference fix (fix tasks); verify tasks have change.patch, left uncommitted
alternatives: [fix-alt.patch]
checks:                            # hidden mdh flows
  fail_to_pass: [checks/recent-search.yaml]
  pass_to_pass: [nowinandroid/regression]   # the app's shared regression flows
private: false
```

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
- **Conditions**: the grader puts the device in the task's condition before the checks (time zone, dark mode,
  font scale, rotation, display size, locale) and back afterwards, through `mdh flow` setup options or adb.
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
- **Unchecked claims**: PASS / FIXED answers from runs that, after their last code change, never installed the
  app and launched it on the device (from the transcript: `adb install`, `am start`, mdh's `run` or `app`,
  mobile-mcp's install or launch).
- **Cost**: tokens, tool calls, screenshots, time; medians per run.

## 7. Size, budget, statistics

- **Pilot**: 20 tasks on Now in Android, five per level (two verify pairs and one fix task each). Calibration
  (60 runs, agent alone) and one mdh run each (20): 80 runs, about 10 hours. It answers whether the levels hold.
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
  tasks stay private so later models can't have trained on them.
- Every workspace is a fresh repository without the app's history.
- Limits: one agent (Claude Code), one device model, mostly one app until more are added. The author of the
  tasks also builds mdh; the review step (3.4) and the adb-only rule are the mitigation, publishing everything
  the check.

## 9. Pilot tasks

Levels are hypotheses for calibration; "real" tasks re-inject the upstream bug.

| # | Level | Kind | Bug or change | Note |
|---|---|---|---|---|
| 1–2 | L0 | verify pair | Settings "Light" and "Dark"; the broken change maps both to dark | |
| 3–4 | L0 | verify pair | Following from the topic screen; the broken change inverts the toggle | |
| 5 | L0 | fix | Bookmarking an article bookmarks another one (wrong id) | |
| 6–7 | L1 | verify pair | Card metadata "date • type"; the broken change drops the blank-type check | 2 of 311 articles show a dangling "•" |
| 8–9 | L1 | verify pair | Search needs 2 characters after trimming; the broken change counts spaces | |
| 10 | L1 | fix, real #1222 | A blank query is saved as a recent search | may calibrate as L0: one `isBlank` |
| 11–12 | L2 | verify pair | Dates in the reader's time zone; the broken change formats in UTC | invisible in Los Angeles, visible east of UTC+1 |
| 13–14 | L2 | verify pair | Keep the settings dialog open across rotation (#611); the broken change uses `remember` | |
| 15 | L2 | fix, real #1864 | The snackbar sits under the keyboard | |
| 16–17 | L3 | verify pair | Feed padding in the design system; the broken change also puts the Interests list under the app bar | |
| 18–19 | L3 | verify pair | Precomputing followed topics; the broken change does it in composition for every card | jank while scrolling |
| 20 | L3 | fix, real #232 | Dark system bar icons in dark mode | |

## 10. Work before the pilot

- mdh-bench: apps (`bench/apps.yaml`: repository, commit, variant, package, JDK), tasks per app, the device
  state of section 4 set and checked, conditions for the grader, `grader_error` with one rerun, the three-way
  answers, unchecked-claim detection, a `regrade` command, per-level and per-source reports.
- mdh: verdict ERROR, not FAIL, when a step fails for infrastructure reasons (found in version 1's runs);
  conditions in a flow's setup (time zone, dark mode, font scale, rotation, display size, locale), useful to users
  as well.
- The device: 4 GB RAM for the AVD.

## 11. Open questions

- Version 1's results keep version 1's prompt (no UNVERIFIED); they aren't comparable to version 2's.
- The second app, and when.
- Who reviews tasks (3.4) besides the author.
