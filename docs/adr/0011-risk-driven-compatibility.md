# ADR-0011: Compatibility is verified risk by risk, not cell by cell

- Status: Accepted (2026-10-03)
- Supersedes in part: [ADR-0009](0009-verification-engine.md) ("compatibility is a matrix")

## Context
Android apps break differently by OS version (behavior changes, API-level branches, new permissions), by device type
(tablets, foldables, cars, TVs), by vendor (background restrictions, permission dialogs, missing system screens on
Xiaomi, OPPO, vivo, Huawei, Honor, Samsung ROMs) and by screen size. ADR-0009 planned a matrix: axes in `mdh.yaml`
expand into cells, and every flow runs on every cell. That is what device farms do, and it fails agents twice:
a useful matrix is large (versions × form factors × configurations), so it is slow and expensive, and most of its
cells test nothing the change could have broken; and a green matrix still says nothing about the vendor ROMs it
didn't include.

The change itself says what is at risk. A new `SDK_INT >= TIRAMISU` branch needs API 32 and 33; a raised
`targetSdk` brings that version's behavior changes; a new `layout-sw600dp` needs a tablet-sized screen; a change to
background work is what vendor ROMs kill. Impact analysis (ADR-0010) already finds the changed declarations and
the screens they reach.

## Decision
Compatibility is a pipeline from the change to verdicts per risk:

1. **Impact** (`mdh-impact`, unchanged in spirit): changed declarations, the screens they reach, and the facts
   compatibility needs, extracted by syntax — names a declaration uses, API levels it compares `SDK_INT` with or
   requires, resource qualifiers, manifest attributes, `minSdk`/`targetSdk`/`compileSdk` before and after.
2. **Risk analysis** (`mdh-compat`, no device, milliseconds): rules over those facts and a **knowledge base**
   shipped as data — Android behavior changes by API level and target SDK, form-factor triggers, vendor quirks —
   each entry with a source link. A risk has a dimension (OS version, device type, vendor, screen size), the reason
   and evidence (`file:line`), the screens it touches, a likelihood, and how to verify it, or why it can't be here.
3. **Verification plan**: the fewest cells that cover the risks, cheapest first. A cell is a device requirement
   (API level, form factor, vendor) plus a configuration (screen size and density override, orientation, font
   scale, dark mode, locale). Configuration on an existing device costs seconds; another emulator costs a boot;
   another system image costs a download. The plan states its cost before anything runs; starting emulators
   (at most two new ones by default) and downloading system images need the user's consent, as everywhere else.
4. **Execution and report**: on each cell, the flows that pass the risk's screens (or the screens reached by deep
   link) run with their checks — functional, UI rules, and for device-type risks a state check across rotation and
   folding. The report is per risk: verified, failed (with the cell and evidence), or **unverified, with what is
   missing** ("needs a Xiaomi device"). An unverified risk is never reported as a pass.

The explicit matrix stays as an escape hatch (`mdh compat run --cells …`) for release testing, but it is not the
default and not what agents are steered to.

Scope of the first version: risk analysis for all four dimensions; screen-size, orientation and configuration
cells on one device; API-level cells on local AVDs; tablets and foldables by display override, and real folding on
foldable AVDs; vendors on connected physical devices, otherwise reported unverified with the knowledge base's hints.
Cars and TVs are reported as risks only. Android Lint (`NewApi` and friends) is an optional static layer, off by
default because it needs a Gradle run.

## Alternatives considered
- **A full matrix per change** (ADR-0009's plan, device farms): coverage by brute force; slow, costly, and blind to
  whatever the matrix leaves out. Kept for release testing only.
- **Pairwise reduction of the matrix**: fewer cells, still unrelated to what changed.
- **Let the model decide what to test**: it would need the knowledge base in its context, every time, and it
  forgets the version boundaries; rules over facts are cheaper and repeatable.
- **Lint only**: precise for API availability, silent on behavior changes, layouts and vendors, and it needs a
  build.

## Consequences
- The knowledge base is part of the product and needs upkeep with every Android release and vendor ROM change;
  entries carry sources so they can be checked. It errs on the side of reporting a risk: a risk costs a cell, a
  missed one is a false pass.
- Name-based matching (a declaration calls `setExactAndAllowWhileIdle`) inherits impact's over-approximation;
  risks say which name matched, so a reader can dismiss a wrong one.
- Display overrides (`wm size`, `wm density`) produce tablet-like configurations on a phone emulator: resource
  qualifiers and window size classes follow, but hardware differences (hinge, multiple displays, a car's
  distraction rules) don't. The report says which cells were simulated.
- Vendors stay the weakest dimension until a cloud device provider exists; the report keeps that visible.
