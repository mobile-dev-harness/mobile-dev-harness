# ADR-0007: Ship the on-device helper in M1

- Status: Accepted (2026-10-02)
- Amends: roadmap in DESIGN.md (helper was M5); architecture §10

## Context
The architecture planned to start on the plain adb backend and pull the helper forward if M1 measurements showed
`uiautomator dump` to be a blocker. Measured on an API 36 emulator (Pixel 9 Pro XL image):

| Operation | Latency |
|---|---|
| `uiautomator dump` | ~2,000 ms every call (4,000 ms on the first), unchanged with animations off or `--compressed` |
| `screencap -p` | ~170 ms |
| `adb shell true` round trip | ~30 ms |

The cost is fixed: each dump starts a process and connects a new UiAutomation session. With it, every act + observe
takes at least 2 s and a stability check (two consecutive trees) 4 s, and we would be no faster than mobile-mcp,
which uses the same command.

## Decision
Ship a minimal helper in M1 and make it the default backend for UI trees, idle waits and Unicode text input.
`uiautomator dump` remains the fallback when the helper can't run.

## Measurements with the helper
| Operation | Latency |
|---|---|
| `mdh observe`, helper already running | ~23 ms end to end (~14 ms for the tree) |
| `mdh observe`, cold (install check, start, forward) | ~360 ms |
| Restart after the helper was killed | ~300 ms |

Compressed output is identical to the `uiautomator dump` snapshots of the same screen.

## Consequences
- The repository contains a small Java Android project (`android-helper/`, no dependencies, ~10 KB APK) signed with
  a deliberately public key, and the prebuilt APK is embedded in the `mdh` binary.
- One UiAutomation client per device: while the helper runs, other clients' `uiautomator dump` calls are killed
  (observed), so mobile-mcp, Appium or Maestro sessions on the same device conflict with it. mdh needs a way to stop
  the helper (`session reset`) and documentation of the conflict.
- M5 keeps the remaining helper work: event streaming, screenshots through the helper, connection pooling.
