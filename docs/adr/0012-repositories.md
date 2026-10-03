# ADR-0012: One engine repository; data, the benchmark and reusable parts in their own

- Status: Accepted (2026-10-03)

## Context
The project moved to the `mobile-dev-harness` GitHub organization. Related projects split their work across many
repositories (Mobile Next: a device agent per platform, a CLI, an MCP server, a test framework, a protocol spec),
largely along language lines. mdh is one Rust workspace whose crates are already modules with a fixed dependency
direction (architecture §2); a few parts don't fit it: the compatibility knowledge base is data with its own
rhythm, the benchmark should be neutral, the on-device helper is Java built with Gradle, and impact analysis is
useful without a device.

## Decision
- **The engine stays in `mobile-dev-harness`**: CLI, MCP server, every engine crate, the Claude Code plugin, the
  sample app and the e2e job. Check kinds share the `Check` interface and the verdict; features cut across crates.
- **A part gets its own repository** when it has users without mdh, a different toolchain, its own release rhythm,
  or must be neutral. In order: `android-compat-kb` and `mobile-agent-bench` (phase 1); `android-helper`,
  `android-impact` and `setup-mdh` (phase 2); `skills`, `ios-helper` (phase 3); `homebrew-tap` whenever releases
  need it. DESIGN.md lists them.
- **Data a binary needs is vendored, pinned and checksummed**: `crates/mdh-compat/kb/` holds a release of
  `android-compat-kb` with its version and SHA-256; `scripts/update-kb.sh <version>` replaces it, and a test fails
  if the snapshot was edited in place. Builds never fetch anything. `MDH_COMPAT_KB` points a binary at another copy
  to try entries before a release.
- **Separate repositories release on their own** and say which mdh versions they work with; mdh says which
  versions of them it was built and tested with.

## Alternatives considered
- **Everything in one repository**: simplest, but the knowledge base's contributors would need the whole Rust
  workspace, and a benchmark shipped with the tool it measures is easy to doubt.
- **One repository per crate** (as with a language split): versioning and cross-repository changes for every
  feature, for no consumer that needs a crate alone yet. Crates can be published from the workspace when one does.
- **Git submodules for the knowledge base**: pinned too, but every clone and CI checkout has to remember them.
- **Fetching the knowledge base at build time**: breaks offline and packaged builds.

## Consequences
- Knowledge-base changes land in `android-compat-kb` first and reach mdh with a snapshot update; the snapshot test
  enforces the order.
- The benchmark repository needs its own pinned copy of the sample app, so its tasks don't break when the sample
  changes for other reasons.
- Each repository needs its own CI, license and README; the organization profile lists them.
