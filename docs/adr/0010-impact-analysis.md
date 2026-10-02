# ADR-0010: Change impact analysis is static, syntax-level and fast

- Status: Accepted (2026-10-03)

## Context
After an edit, an agent has to decide what to verify. Today it guesses from the diff it just wrote: it checks the
screen it was working on and misses the other screens that call the function it changed, the layout it touched,
the string it reworded. That is the false pass "fixing screen A breaks screen B", and it is cheap to prevent if
something tells the agent what the change reaches. Reading the whole project to find out costs far more tokens
than the change itself.

The answer has to be cheap enough to compute after every edit (well under a second on a large app), work on code
that doesn't compile yet, and say how sure it is.

## Decision
`mdh impact` compares the working tree with a base (default `HEAD`, `--base <ref>` for a branch or commit) and
reports, without a device and without building:

1. **What changed, semantically**: added, removed and modified declarations (classes, functions, properties,
   Android resources, manifest entries), a signature change told apart from a body change; changes to comments and
   formatting only are dropped.
2. **Before → after**: calls, navigation edges and resource references the changed code gained or lost.
3. **Impact**: callers and users of what changed, followed up to the screens (activities, fragments, composables
   hosted by an activity) that show it, with the path, and how to reach each screen (deep link, or taps from the
   launcher screen).
4. **What to verify**: screens for functional checks, UI checks when layouts, resources or composables changed,
   performance hints (list adapters, lazy lists, drawing, startup), compatibility hints (manifest, qualified
   resources, API-level branches), unit tests that reference the changed code.

How:

- **tree-sitter, syntax level.** Kotlin (`tree-sitter-kotlin-ng`), Java and XML are parsed into declarations and
  references; there is no type checker. References are resolved by name with what syntax does tell: the
  receiver's declared type, imports, the package, the enclosing class. Every resolved edge carries a confidence:
  `exact` (one candidate, confirmed by type, import or scope), `likely` (one candidate by name),
  `ambiguous` (several).
- **Never claims "no impact".** What syntax can't see — reflection, dependency injection, generated code, string
  routes built at run time — is stated in the output as a blind spot, so an empty result reads as "nothing found",
  not "nothing affected".
- **Index everything, every time.** The whole project is parsed on each run (Now in Android: 310 Kotlin files,
  1 MB, ~180 ms on one core); only changed files are parsed twice (the base version comes from
  `git cat-file --batch`). No cache to invalidate. A cache is the escape hatch if a project ever needs one.
- **Budgeted output.** Text output follows the same rules as everything else (principle 5): each section has a
  line budget and says what it folded.

The analysis lives in its own crate, `mdh-impact`, next to `mdh-observe` (the runtime view of the app; this is the
static view of the source). The verification engine (M4) uses it to pick the flows and checks a change needs.

## Alternatives considered
- **The Kotlin compiler or its analysis API**: precise types, but it needs a configured Gradle build (tens of
  seconds, and not at all while the code doesn't compile), a JVM and per-AGP/Kotlin-version maintenance.
- **A language server** (kotlin-language-server, JetBrains' Kotlin LSP): same configuration cost and a resident
  process; startup on a large project takes minutes.
- **Module-level analysis from Gradle**: too coarse — every change in `:app` would affect every screen.
- **Regular expressions**: can't tell a declaration from a call, a comment from code, or one scope from another.
- **Let the model read the diff and grep**: what agents do today; it costs many tool calls and tokens and stops
  at the first level of callers.

## Consequences
- Name-based resolution over-approximates: overloaded or common names can pull in unrelated callers. They are
  labeled `ambiguous` and propagated at most one level, so they widen the list without flooding it.
- Only one Kotlin grammar can be linked into the binary: tree-sitter grammars export fixed C symbols
  (`tree_sitter_kotlin`), and two grammars silently share one of them. `tree-sitter-kotlin-ng` was chosen after
  measuring both separately (Now in Android: 0 of 310 files with parse errors, against 11 for
  `tree-sitter-kotlin-sg`).
- Both grammars misparse calls to functions named like a soft keyword (`open(…)`, `value(…)`). Files with parse
  errors are parsed again with those names masked (same length, so byte offsets still point into the original
  text), and the attempt with fewer errors wins.
- Kotlin and Java first; other languages (Dart, TypeScript, Swift) plug in as further extractors when their
  platforms arrive (M8, M9).
