---
name: verify
description: Verify a change to an Android app on a device or emulator before calling it done — find the affected screens, build and run, drive each screen, and get a passing verdict with evidence. Use after editing app code, layouts, resources or the manifest, and whenever asked to check that something works.
---

# Verify an Android change

A change is done when it has a **passing verdict**, not when it compiles. The mdh MCP tools do the
work; the same steps exist as `mdh` CLI commands.

1. **Impact.** Call `mdh_impact`. It lists the changed declarations, the screens they reach and how
   to get to each one (deep link, or the taps from the launcher), call sites that no longer fit a
   changed signature, what else to check (UI, performance, compatibility), the tests that use the
   code, and the saved flows that pass those screens. Fix broken call sites first.
2. **Build and run.** Call `mdh_run`; build errors come back as `file:line`, fix them and run
   again. If no device is online, the error lists the emulators that can be started: ask the user
   before starting one (it takes a while and uses memory), then call `mdh_status` with
   `start_emulator`. If several are online, ask which to use and pass it to `mdh_status` as
   `device` (remembered for the project). With a phone and an emulator connected, the emulator is
   used.
3. **Drive each affected screen,** not only the one you edited. Open it with `mdh_app`
   (`command: open` with the deep link) or follow the taps impact showed. `mdh_observe` shows the
   screen as a compact tree with refs (`e12`); `mdh_act` taps, types, scrolls and presses keys, and
   each action returns only what changed. Prefer refs and labels over coordinates; never guess
   coordinates from a screenshot.
4. **Check.** Call `mdh_verify` with the checks that prove the change works, for example
   `screen .LoginActivity`, `enabled id=sign_in`, `text id=title == "Inbox"`, `not visible id=error`.
   `no crash` is always checked. A failed check shows what was observed; evidence (screenshot, tree,
   logs) is in the run directory the verdict names. Fix and verify again until it passes.
5. **Regressions.** Call `mdh_verify` with `changed: true` to replay the saved flows that pass the
   affected screens. When you verified something worth repeating, save it with `mdh_flow`
   (`command: save`, `name`, and the `checks` to run at the end; `last` keeps only the last N steps).

Report the verdicts in your answer. If something can't be verified here (no device, needs real
accounts or hardware), say exactly what wasn't verified instead of implying it works.

Rules of thumb:

- Lines starting with `!!` are crashes or ANRs of the app: fix them before anything else.
- `obscured` elements are covered by system bars or the keyboard: that's usually a layout bug.
- A screen still loading is fine: checks wait up to 3 s for the expected state.
- Typed passwords are saved in flows as `${env:MDH_<FIELD>}`; set the variable before replaying.
