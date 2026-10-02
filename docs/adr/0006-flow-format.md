# ADR-0006: Own YAML flow format, Maestro import later

- Status: Accepted (2026-10-02)

## Context
We could adopt Maestro's flow format and borrow its ecosystem and user familiarity, or define our own.

## Decision
Use our own YAML format (functional design §4.6), with syntax kept close to Maestro's. Later (F7.5), provide an
importer for the common Maestro subset.

## Rationale
- Flows are tightly bound to our own concepts: state setup in `setup` (reset, snapshots, routes), secret
  references, the assertion model, and a one-to-one mapping with the recording format.
- Maestro compatibility would mean tracking its semantics as it evolves, and partial compatibility confuses users.
- An importer lowers migration cost more realistically than full compatibility.

## Rejected alternatives
- **Full Maestro compatibility:** large ecosystem benefit, but our differentiators (state, evidence, recording) are
  hard to fit into its format.

## Consequences
- We maintain our own format documentation and JSON Schema (generated from Rust types, for editor completion and
  validation).
