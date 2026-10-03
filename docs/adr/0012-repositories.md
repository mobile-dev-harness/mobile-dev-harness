# ADR-0012: Code in one repository, per platform by directory; data and the benchmark in their own

- Status: Accepted (2026-10-03)

## Context
The project moved to the `mobile-dev-harness` GitHub organization. Related projects split their work across many
repositories (Mobile Next: a device agent per platform, a CLI, an MCP server, a test framework, a protocol spec),
largely along language and platform lines. Splitting by platform × capability multiplies repositories with every
platform: an iOS port would add a helper, an impact analyzer and more. mdh is one Rust workspace whose crates are
already modules with a fixed dependency direction (architecture §2); features cut across them, and the check kinds
share one `Check` interface and one verdict.

Two parts don't fit that: the compatibility knowledge base is data with its own rhythm (Android releases, ROM
changes) and its own contributors, and a benchmark shipped with the tool it measures is easy to doubt.

## Decision
- **Code lives in `mobile-dev-harness`, platforms by directory.** The CLI, the MCP server and every engine crate;
  the on-device helpers (`android-helper/` today, `helpers/<platform>/` once there is a second); impact analysis
  (`mdh-impact`, its parsers by language); integrations for agents (`integrations/claude-code/`, more agents next
  to it); a GitHub Action (`action.yml` at the root). A crate that gains users of its own is published from the
  workspace, not moved.
- **Only data and neutral ground get their own repository**: `compat-kb` (the knowledge base, one file per
  platform: `android.yaml` first) and `mobile-agent-bench` (tasks per platform by directory, the runner, the
  graders, results). `homebrew-tap` follows when releases ship through Homebrew, whose naming requires it.
- **Data a binary needs is vendored, pinned and checksummed**: `crates/mdh-compat/kb/` holds a release of
  `compat-kb` with its version and SHA-256 in `kb/SOURCE`; `scripts/update-kb.sh <version>` replaces it, and a
  test fails if the snapshot was edited in place. Builds never fetch anything. `MDH_COMPAT_KB` points a binary at
  another copy to try entries before a release.
- **Separate repositories release on their own** and say which mdh versions they work with; mdh says which
  versions of them it was built and tested with.

## Alternatives considered
- **A repository per platform and capability** (helpers, impact analysis, an Action, skills, each per platform):
  nine repositories for one maintainer, cross-repository changes for most features.
- **Everything in one repository**: the knowledge base's contributors would need the whole Rust workspace, and the
  benchmark would ship with the tool it measures.
- **Git submodules for the knowledge base**: pinned too, but every clone and CI checkout has to remember them.
- **Fetching the knowledge base at build time**: breaks offline and packaged builds.

## Consequences
- Knowledge-base changes land in `compat-kb` first and reach mdh with a snapshot update; the snapshot test
  enforces the order.
- The benchmark repository keeps its own pinned copy of the sample app, so its tasks don't break when the sample
  changes for other reasons.
- A second platform adds directories, not repositories.
