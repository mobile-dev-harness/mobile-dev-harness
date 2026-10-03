---
name: compat
description: Verify an Android change's compatibility risks with mdh — other API levels, tablets and foldables, rotation and saved state, vendor ROMs, screen sizes — on the fewest devices and configurations that show them. Use when impact lists compatibility items, or the change touches SDK_INT checks, targetSdk, qualified resources, saved state or background work.
---

# Compatibility checks

```sh
mdh compat risks                 # the risks, with evidence and where each would show (no device)
mdh compat plan                  # the devices and configurations that would verify them
mdh compat run [--no-start]      # verify: a verdict per risk
```

- Screen sizes, landscape and saved state are checked on the current emulator (display overrides, restored
  after). Other API levels need another AVD: if the plan has to start one, `run` fails with `NEEDS_CONSENT`;
  **ask the user**, then `mdh compat run --yes`, or `--no-start` to leave those cells out.
- Each risk comes back failed (where, and what broke), passed (where), or unverified with what's missing (a
  vendor device, a flow through the screen). Report unverified risks as unverified, never as passed.
- A risk without a flow through its screen stays unverified: save one with the mdh flow tool.
- Without a shell, the same is the `mdh_compat` tool (`mdh mcp --tools all`).
