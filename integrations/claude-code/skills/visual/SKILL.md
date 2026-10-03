---
name: visual
description: Check the UI consistency of an Android screen with mdh — layout and accessibility rules, structural and pixel baselines, larger font, dark mode, right to left. Use when a change touches layouts, styles, strings or composables, or the user asks about how a screen looks.
---

# UI consistency checks

Run them with the `mdh` CLI on the screen the session shows (open it first with the mdh tools):

```sh
mdh visual check                                   # rules: touch targets, labels, overlap, obscured, duplicate labels, contrast
mdh visual check --baseline login                  # also compare with (or record) the baseline named login
mdh visual check --configs font_scale,dark,rtl     # the same screen at font scale 1.3, in dark mode, right to left
mdh visual check --baseline login --ignore id=clock  # leave dynamic elements out of the comparison
mdh visual approve login                           # accept the deviations as the new baseline
```

- Exit code 1 is a failed check; the verdict says what moved, resized, appeared or disappeared, which pixel
  regions changed, and where the diff image is.
- The first `--baseline` run records the baseline (a warning, not a failure): commit `.mdh/baselines/`.
- Approve only deviations the change meant to make; otherwise fix the layout.
- Flows run these checks too when their YAML has a `visual:` section.
- Without a shell, the same is the `mdh_visual` tool (`mdh mcp --tools all`).
