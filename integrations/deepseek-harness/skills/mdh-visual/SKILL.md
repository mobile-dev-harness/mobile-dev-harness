---
name: mdh-visual
description: Check Android UI consistency with mdh rules, structural and pixel baselines, and font scale, dark mode or RTL variants. Use for layout, style, string or composable changes and reported visual regressions.
---

# Check Android UI consistency

Use the shell for these specialist checks; the default MCP connection exposes
only eight core tools. CLI `.mdh/session.json` is separate from MCP. Work in the
same project, explicitly select the device and app, then navigate to the intended
screen using CLI actions before checking. Replace the example paths and package.

```sh
cd /absolute/path/to/android-project
mdh --device SERIAL launch com.example.app
mdh --device SERIAL observe
# Navigate to the intended screen with CLI actions before checking.
mdh --device SERIAL visual check --baseline login --ignore id=clock --configs font_scale,dark,rtl
```

Read failed rules, observed geometry and evidence. Inspect screenshots for images
and pixel differences. Compare the same device profile; ignore only known dynamic
content, not the element under test. A first named-baseline run creates a baseline,
not proof of no regression. Review `.mdh/baselines/` with the app change.

Approve only reviewed, intended deviations, scoped to that baseline:

```sh
mdh --device SERIAL visual approve login
```

Explicitly opt in to `tools: all` for a client without a shell, or when specialist
checks should share the active MCP session; preserve the remaining profile config.
DSH prefixes MCP tool names with `mcp__mdh__` by default. The examples name the
underlying tool; resolve its actual discovered name if the server was customized.
Navigate with the same MCP session before using this alternative:

```mcp
{"tool":"mdh_visual","arguments":{"command":"check","baseline":"login","ignore":["id=clock"],"configs":["font_scale","dark","rtl"]}}
```

The equivalent scoped approval, after reviewing the candidate:

```mcp
{"tool":"mdh_visual","arguments":{"command":"approve","baseline":"login"}}
```

Configuration variants should restore device settings. Inspect the resulting
state after an interruption. Normal `mdh_verify` flow replay supports `visual:`;
a compatibility run does not automatically execute that section. Preserve existing
authorization for device use or emulator startup.
