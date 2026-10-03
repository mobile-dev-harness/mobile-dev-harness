---
name: debug-crash
description: Find and fix the cause of an Android app crash, native crash or ANR reported by mdh (lines starting with "!!", APP_CRASHED, or a failing "no crash" check), then prove the fix with a verdict.
---

# Debug a crash

1. **Read the report you already have.** mdh puts the crash in the result: the exception, the
   app's own stack frames first (framework frames folded), `caused by:` lines, and the steps that
   led there (`after: …`). Call `mdh_observe` with `logs: "info"` for the surrounding log lines if you need more.
2. **Find the cause in the code.** Start at the deepest app frame of the root cause (the last
   `caused by:`), not at the first frame. For an ANR, look for work on the main thread (I/O, locks,
   long loops, `Thread.sleep`) in the code the last action triggered. For a native crash (signal),
   look at the app's own library frames and what Java/Kotlin code called into them.
3. **Reproduce it.** Replay the steps from the report with `mdh_act` (or the flow that failed with
   `mdh_verify` and `flows`), so you know the fix is tested on the same path.
4. **Fix, then prove it.** `mdh_run`, repeat the steps, and `mdh_verify` with the checks the screen
   should pass; `no crash` is always part of the verdict. Run `mdh_impact` to see which other
   screens the fix reaches and verify them too.
5. **Keep it fixed.** Save the reproduction as a flow with `mdh_flow` (`command: save`) so the crash
   can't come back unnoticed.
