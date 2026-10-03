# ADR-0012: Repositories by layer — controller, analyzer, product; platforms by directory

- Status: Accepted (2026-10-03)

## Context
The project moved to the `mobile-dev-harness` GitHub organization. Related projects split their work across many
repositories (Mobile Next: a device agent per platform, a CLI, an MCP server, a test framework, a protocol spec),
along language and platform lines. Splitting by platform × capability multiplies repositories with every platform.
Keeping everything in one repository hides a real boundary, though: mdh has three layers with different users.

- The **controller** drives devices: types and errors, drivers and on-device helpers, compact UI trees, logs and
  crashes, sessions and actions, builds. Anyone giving an agent a device can use it without the rest.
- The **analyzer** reads code without a device: what a change reaches, its compatibility risks. CI, code review
  and other agents can use it on its own.
- The **product** turns both into verdicts: the verification engine, the check kinds, the CLI and MCP server.

Two parts are neither: the compatibility knowledge base is data with its own rhythm and contributors, and a
benchmark shipped with the tool it measures is easy to doubt.

## Decision
- **Three code repositories, one per layer**: `controller` (`mdh-core`, `mdh-driver`, `mdh-observe`,
  `mdh-control`, `mdh-project`, the helpers), `analyzer` (`mdh-impact`, `mdh-risk`) and `mobile-dev-harness`
  (`mdh-cli`, `mdh-mcp`, `mdh-verify`, `mdh-visual`, `mdh-perf`, `mdh-compat`, the agent integrations, the sample
  app, e2e). Dependencies point one way: the product depends on the controller and the analyzer; they depend on
  neither each other nor it. The analyzer's errors are its own; the product converts them
  (`mdh_verify::impact_error`).
- **Platforms are directories, not repositories**: `helpers/<platform>/`, driver modules, parsers by language,
  `integrations/<agent>/`.
- **Data and neutral ground get their own**: `compat-kb` (one file per platform) and `mobile-agent-bench` (tasks
  by platform, the runner, graders, results). `homebrew-tap` when releases ship through Homebrew.
- **Pinned, never fetched at build time**: the product depends on the controller and the analyzer by git tag
  (crates.io later); `mdh-risk` vendors a `compat-kb` release with its SHA-256 in `kb/SOURCE`, replaced by
  `scripts/update-kb.sh <version>`, checked by a test. Local development across repositories uses Cargo
  `[patch]`. `MDH_COMPAT_KB` points a binary at another copy of the knowledge base.
- **The split waits for the 2026-10 benchmark run**; until then the crates live in `mobile-dev-harness`, with the
  layer boundaries already enforced by their dependencies.

## Alternatives considered
- **A repository per platform and capability** (helpers, impact analysis, an Action, skills, each per platform):
  nine repositories for one maintainer, cross-repository changes for most features.
- **Everything in one repository, platforms by directory**: simplest, but the controller and the analyzer have
  users of their own who would have to take the whole product.
- **Git submodules** for the knowledge base or the layers: pinned too, but every clone and CI checkout has to
  remember them.

## Consequences
- A feature that needs a new controller capability lands in `controller` first, is tagged, then used in the
  product; `[patch]` keeps that cheap while developing.
- The benchmark repository keeps its own pinned copy of the sample app, so its tasks don't break when the sample
  changes for other reasons.
- A second platform adds directories, not repositories.
