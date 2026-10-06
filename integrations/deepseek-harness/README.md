# mobile-dev-harness for DeepSeek Harness

A DSH bundle that connects the existing Rust `mdh mcp` server and adds five Android
verification skills. Device control, impact analysis, flows and verdicts remain in
mdh; this package only adapts the DSH plugin lifecycle and configuration.

## Requirements and compatibility

- An installed `mdh` **0.4.0 or newer** executable and the Android prerequisites described
  in the [main README](https://github.com/mobile-dev-harness/mobile-dev-harness#requirements). Existing binary releases
  support macOS and Linux. This integration does not bundle or download mdh.
- Node.js **`^22.19.0 || >=24.0.0`** and a compatible DSH installation.
- DSH must supply `@deepseek-ai/dsh-mcp-client` and
  `@deepseek-ai/dsh-skill-filesystem`. Their declared peer range is
  **`^0.1.1-rc.2`**. They are marked optional for package installation to avoid
  pulling in a second host installation; both are required at runtime.

The adapter targets the APIs in DSH **`v0.1.1-rc.2`**, commit **`b150a551`**. DSH
is a prerelease host: the peer range is not a guarantee that every later release
is compatible. Test upgrades against the intended DSH installation. The local
smoke test below exercises the installed host APIs without an Android device.
At startup the adapter checks the selected executable's version before connecting
MCP. Missing, unreadable or older binaries fail with an actionable startup error.

## Install a local package

This package is not published to npm yet. From this repository:

```sh
cd integrations/deepseek-harness
pnpm pack
dsh plugin --profile web add /absolute/path/to/dsh-mobile-dev-harness-0.1.0.tgz
```

Use the tarball path printed by `pnpm pack`. Packing runs the dependency-free
configuration checks before including the adapter, skills and license files.
There is no Rust or TypeScript build inside the package.

Use DSH's shipped `web` profile, which includes the chat interface. The bundle
inserts a **disabled** plugin row. Enable it only after binding it to an Android
project. Add this override to `$DSH_HOME/profiles/web/cordis.patch.yml`.
`DSH_HOME` defaults to `~/.dsh`:

```yaml
- id: mobile-dev-harness
  disabled: false
  config:
    project: /absolute/path/to/android-project
    command: mdh
    device: emulator-5554
    tools: core
```

Replace the paths and device serial. `command` can be an absolute executable path
if mdh is not on DSH's `PATH`; relative paths and `~` expansion are not supported.
Omit `device` to use mdh's existing device resolution. The plugin does not start
an emulator when it loads. Missing or ambiguous devices are reported when a tool
needs a device.

Inspect the composed configuration, then start DSH from the same Android project:

```sh
dsh --profile web --dump-config
cd /absolute/path/to/android-project
dsh web
```

DSH's workspace defaults to its launch directory; `config.project` only sets the
mdh subprocess directory. If selecting a workspace in the UI, choose the same
Android project. Changing the UI workspace does not retarget this fixed mdh
connection: update `config.project` and reconnect before verifying another project.

DSH overrides replace the **entire `config` object**, rather than merging its
keys. Include every setting you need in the final override. Home-level patches
and command-line overlays can also override the profile.

## Configuration

| Field | Default | Meaning |
| --- | --- | --- |
| `project` | Required | Existing absolute Android project directory; used as the MCP process's working directory. |
| `command` | `mdh` | Executable name on `PATH` or absolute executable path. |
| `device` | mdh resolution | adb serial or running AVD selection, passed as `mdh --device …`. |
| `tools` | `core` | `core` exposes 8 tools; explicit `all` opt-in exposes 11. |
| `serverName` | `mdh` | Tool namespace; 1–32 letters, digits, underscores or hyphens. |
| `env` | `{}` | Explicit environment variables forwarded to the MCP process. |
| `toolCallTimeoutMs` | `600000` | Tool timeout in milliseconds; allows long builds and verification runs. |

DSH's MCP client filters sensitive inherited environment variables. Forward a
flow's required secret explicitly from the DSH process environment instead of
embedding it in a committed configuration. For example, after setting
`MDH_PASSWORD` in the environment used to start DSH, use this complete override:

```yaml
- id: mobile-dev-harness
  disabled: false
  config:
    project: /absolute/path/to/android-project
    command: /absolute/path/to/mdh
    device: emulator-5554
    tools: core
    env:
      MDH_PASSWORD: !!js process.env.MDH_PASSWORD
```

Every `env` value must resolve to a string. If a variable is unset, set it before
starting DSH or remove the unused entry. SDK or Java environment variables can
also be forwarded explicitly when the host environment requires it.

## Tools, skills and session boundaries

With `serverName: mdh`, tool names are prefixed with `mcp__mdh__`, for example
`mcp__mdh__mdh_verify`. Follow the actual discovered names if customized.

| Tool set | Tools |
| --- | --- |
| `core` (default) | `mdh_status`, `mdh_observe`, `mdh_act`, `mdh_run`, `mdh_impact`, `mdh_verify`, `mdh_flow`, `mdh_app` |
| Additional tools in `all` | `mdh_visual`, `mdh_perf`, `mdh_compat` |

DSH has a shell, so the specialist skills normally use `mdh visual`, `mdh perf`
and `mdh compat`. This keeps the default MCP tool set small. Set `tools: all`
explicitly for a client without a shell, or when specialist checks should share
the active MCP session. Preserve the other configuration fields when opting in.

The bundled skills are `mdh-verify`, `mdh-debug-crash`, `mdh-visual`, `mdh-perf` and
`mdh-compat`. All tools use the same in-memory mdh session. The configured project
owns flow discovery, baselines and evidence; a per-call `project` argument does
not rebind the MCP process. Reconfigure and restart the connection to change its
project.

Use one workflow per configured connection and device. A globally mounted DSH
plugin can be shared by multiple agents; it does not provide a private Android
session for each agent. Do not concurrently drive the same device, including from
another profile or a separate CLI. CLI `.mdh/session.json` is separate from the
MCP session and does not inherit its app, refs or recorded steps. Before a CLI
specialist check, use the same project working directory, select the device and
app explicitly, and establish the required screen or saved-flow setup in the CLI.

This bundle has no Claude Code Stop hook and does not enforce a completion gate
bound to the current diff. Agents must report the actual checks, evidence and
unverified conditions. Compatibility execution does not automatically run a
flow's `visual:` or `perf:` sections; run those checks separately when needed.

## Development and release

Run the syntax checks and configuration tests without installing dependencies:

```sh
npm run check
```

Validate the annotated MCP examples in both the DSH and Claude Code skills against
the live tool schemas from a compiled mdh binary, from this source checkout:

```sh
DSH_MDH_BINARY=/absolute/path/to/mdh npm run test:contract
```

This contract test needs only mdh; it does not require DSH, a device or network
access. CI runs the checks and contract test automatically. Skill examples use
fenced `mcp` blocks with one JSON object containing `tool` and `arguments`; this
keeps their tool names and parameter shapes checked against the real MCP API.

For a smoke test against an existing DSH checkout and mdh executable:

```sh
DSH_ROOT=/absolute/path/to/deepseek-harness \
DSH_MDH_BINARY=/absolute/path/to/mdh \
npm run test:dsh
```

This test requires the checkout's built DSH packages and existing dependencies;
it does not install them. It packs this integration, installs that artifact offline
in a temporary directory, and checks real host registration, impact analysis,
startup failures and disposal. No emulator or connected Android device is required.
Device interaction, Gradle builds and a full verification flow still need manual
validation in the intended Android environment.

Public release is deferred until the main `mobile-dev-harness` project reaches
**1.0.0**. Until then, keep this integration available for local packaging and
testing only. The package is marked `private: true` to prevent npm publication;
this does not prevent packing or installing a local tarball.

The integration's current `0.1.0` version is for local development, not a public
release. As part of the main project's 1.0.0 release preparation, revalidate DSH
compatibility, confirm npm name availability and publisher authorization, choose
the integration's release version, and remove the private flag when publication
is intended. Reaching 1.0.0 does not trigger an automatic publish.

The package has no install-time download or build script. See the
[DSH packaging documentation](https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/user/develop/basic/publish.md)
for bundle installation and layer precedence.

To remove the bundle and its layer:

```sh
dsh plugin --profile web remove dsh-mobile-dev-harness
```

Remove the now-unused `mobile-dev-harness` override from the profile patch too.
