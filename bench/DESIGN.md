# Benchmark design, version 2

Status: draft for the pilot. Version 1 (the ten tasks in `tasks/`) stays as the L0 control set.

## What it measures

**When a coding agent says an Android change works, can you believe it?** Version 1 answered a narrower
question and could not tell the setups apart: every bug in it was visible in the diff, so the agent alone, reading
code, was right 28 of 28 times (GLM-5.3, 2026-10-04), while the setups with a device only added ways to get stuck.
Version 2 grades the same two things, whether the agent's work is right and whether its claim about it is, on
tasks where the claim needs evidence that only a running app gives.

## Related work

| | Tasks | Oracle | Measures |
|---|---|---|---|
| SWE-bench (2023), Multimodal (2410.03859) | real issues and PRs; Python, JS with images | the PR's unit tests | resolved rate |
| SWE-Bench Mobile (2602.09540) | PRD + Figma features in a production iOS app | test suites, hosted | task success (≤12%) |
| MobileDev-Bench (2603.24946) | 407 real issues, 19 apps (Android, React Native, Flutter) | test patches, containers | resolved rate (3–6%) |
| AppEval (2608.18588) | 200 Android repairs from 24 repos (+ iOS, HarmonyOS) | instrumentation test on the installed app; infrastructure failures a separate outcome | Pass@1 (22–90%) |

All four ask whether the agent's patch resolves the task. None asks whether the agent knows: whether its own
"fixed" or "works" is true, how often it claims success it didn't check, or what device access changes about that.
None has review tasks (judge a teammate's change), and none compares the agent's tooling on the same model.
That is this benchmark's place. The name stays `mobile-agent-bench`; "SWE-Bench Mobile" is taken.

Taken from them: real issues where possible and a reference fix kept apart from the hidden check (all);
FAIL_TO_PASS and PASS_TO_PASS checks (SWE-bench); a task is accepted only if the check fails on the defect and
passes on the fix on the same installed target, and infrastructure failures are their own outcome (AppEval);
human validation of every task (SWE-bench Verified).

## Tasks

Two kinds, as in version 1:

- **Fix**: an issue against the code; the agent fixes it and answers `FIXED`, `NOT FIXED` or `UNVERIFIED`.
- **Verify**: a teammate's uncommitted change and what it should do; the agent answers `PASS`, `FAIL` or
  `UNVERIFIED` without changing code. Verify tasks come in pairs, the same description with a correct and a broken
  change, so a constant answer scores 50%.

### Levels: what evidence decides the task

| Level | Decided by | Examples |
|---|---|---|
| L0 | the code or the diff | an inverted condition; a misspelled package name |
| L1 | running the app, one path | data the code mishandles (2 of 311 demo articles have no type); a runtime-only crash; a library default |
| L2 | running under a condition, or several steps | dark mode, font scale, time zone, rotation or process death, window size, keyboard, back stack |
| L3 | measuring, or looking away from the change | jank, startup time, a few pixels of overlap, an API-level difference, a shared component breaking another screen |

A level is a hypothesis until measured. **Calibration:** every task is run by the agent alone with the strongest
model available. L0 tasks must be solved reliably that way; for L1 and above, the agent alone must be near chance
on verify pairs and must not reach FIXED-and-working more often than by luck. A task that misses its level moves
level or is rewritten. Every task must also be decidable with adb alone (no setup gets a check only its tools can
see) and is checked by `mdh-bench validate`.

### Sources

1. **Real bugs, re-injected.** NiA's tracker has 179 closed bugs, 74 with a linked PR, about a third of them
   observable at runtime (#1864 snackbar under the keyboard, #1295 snackbar missing on wide screens, #1222 empty
   query saved as a recent search, #611 settings dialog lost on recreation, #541 bookmark icon ignores the theme,
   #415 scroll position lost between tabs, #232 dark system bar icons in dark mode). The bug is re-introduced
   into the pinned commit and the issue text kept. Models may have seen the fix: results on this source are
   reported apart.
2. **Synthetic bugs from the taxonomy** in the levels table, with issues written for them. Unseen by any model;
   the level is chosen.
3. **Verify pairs**, from real PRs or written, each with a correct and a broken change.

### Apps

- **Now in Android** (Apache 2.0) at a pinned commit, `demoDebug` (local static data, no backend): 37 Gradle
  modules, 17.5k lines of non-test Kotlin, Compose, Hilt, Room, DataStore; API 23–36, adaptive layouts, dynamic
  color, a benchmark module and screenshot tests. Too large to read whole, so evidence has to come from somewhere.
- **The sample app** (version 1): the L0 control tasks.
- Later, one or two more open-source apps (candidates: Wikipedia, Thunderbird; licenses to check). MobileDev-Bench
  uses Thunderbird, so tasks there must not come from issues it already has.

## Environment

Pinned and recorded in every result: the app commit, JDK 17, the Gradle distribution, the system image and AVD
config (Pixel 9 Pro XL, API 36, 4 GB RAM), and the device identity checked before and after every run.

Builds: a fresh copy of NiA builds in 84 s with warm Gradle caches (27.6 min the first time, downloading
everything), 17 s after an app-only change. A benchmark session builds the pinned commit once to warm the shared
build cache, then builds with `--offline`. The demo flavor still loads article images from the network; checks
don't depend on them.

## Grading

- **FAIL_TO_PASS**: hidden mdh flows that fail on the defect and pass on the reference fix; for verify pairs,
  fail on the broken change and pass on the correct one.
- **PASS_TO_PASS**: flows over the rest of the app (start, the four tabs, settings) that must keep passing, so a
  fix that breaks something else isn't counted as fixed.
- Flows check behavior, never the implementation, and every task is validated against at least one alternative
  correct fix when one exists.
- **Infrastructure failures are not failures.** A flow that ends in ERROR (unreadable screen, device gone) is
  re-run after a device reset; if it errors again the run is recorded as `grader_error` and left out of the rates.
  (Two GLM runs were counted as false passes because the grader couldn't read the screen.)

## Answers and metrics

- `UNVERIFIED` is a legitimate answer: "I made the change but couldn't check it" or "I couldn't decide". It is never
  counted as correct and never as a false pass or false fail.
- **Headline: the false-pass rate**, the share of PASS / FIXED answers that are wrong. Then the false-fail rate,
  the share answered (not UNVERIFIED), the resolved rate (fix tasks whose checks pass, whatever was said), and
  cost: tokens, tool calls, time.
- **Unchecked claims**: FIXED or PASS answers from runs that never installed and ran the app. In version 1 the agent
  alone answered FIXED 12 of 12 times without a device, against the prompt, and was right only because the bugs
  were in plain sight.
- Per level, per source and per task, so one task's weight is visible.

## Size and budget

- **Pilot: 20 tasks on NiA**, five per level (two verify pairs and one fix task each), the agent alone and mdh,
  one model, one run each: 40 runs, half a day. It answers one question: do the levels hold.
- **Full: 100 tasks**, 25 per level (eight verify pairs and nine fix tasks each), one run per task and setup, plus
  three runs of a fixed 20-task subset to measure run-to-run variance. Four setups: 480 runs, about 40 hours per
  model at version 1's pace; the timeout drops from 25 to 15 minutes.
- With 25 tasks per level and paired setups, differences of about 20 points per level are detectable.

## Fairness and contamination

- Bug types come from real trackers, not from what mdh can check.
- Every check is decidable with adb, screenshots and logcat.
- The task set is frozen before a published run; tasks, checks, prompts and agent transcripts are published.
- 20% of the tasks stay private, so later models can't have trained on them.
- Every workspace is a fresh repository without the app's history.

## Pilot tasks (hypotheses; levels set by calibration)

| # | Level | Kind | Bug or change |
|---|---|---|---|
| 1–2 | L0 | verify pair | Settings: "Light" and "Dark" options; the broken one maps both to dark |
| 3–4 | L0 | verify pair | Following from the topic screen; the broken one inverts the toggle |
| 5 | L0 | fix | Bookmarking an article removes the bookmark of another one (wrong id passed) |
| 6–7 | L1 | verify pair | Card metadata "date • type"; the broken one drops the blank-type check (2 of 311 articles show a dangling "•") |
| 8–9 | L1 | verify pair | Search requires 2 characters after trimming; the broken one counts spaces |
| 10 | L1 | fix (real, #1222) | An empty or blank query is saved as a recent search |
| 11–12 | L2 | verify pair | Dates in the reader's time zone; the broken one formats in UTC (articles are published at 23:00 UTC) |
| 13–14 | L2 | verify pair | Keep the settings dialog open across rotation (#611); the broken one keeps it in a non-saveable state |
| 15 | L2 | fix (real, #1864) | The snackbar sits under the keyboard |
| 16–17 | L3 | verify pair | A design-system padding change for the For You feed; the broken one also shifts the Interests list under the app bar |
| 18–19 | L3 | verify pair | Precomputing followed topics; the broken one does it in composition for every card (jank while scrolling) |
| 20 | L3 | fix (real, #232) | Dark system bar icons in dark mode |

## Open questions

- The prompt's answer format changes (`UNVERIFIED`): version 1's results stay with version 1's prompt.
- mdh-bench supports one app today: tasks need an `app` (sample, or NiA at a commit), a variant and a package.
- Which second app, and when.
