# ADR-0002: Own driver instead of depending on mobile-mcp

- Status: Accepted (2026-10-02)

## Context
mobile-mcp already offers device control (screenshots, accessibility tree, taps, input). We could depend on it or
implement the device layer ourselves.

## Decision
Build a thin driver of our own: first wrapping the adb CLI directly, later adding an on-device helper. mobile-mcp
is not a dependency.

## Rationale
- The project's core differentiators — UI tree compression, stable refs, diffs, wait strategy, screenshot policy —
  need direct access to raw data and control over how it is fetched. They are hard to do well behind someone
  else's abstraction.
- Calling another MCP server from inside an MCP server adds a process and a protocol hop and makes failures harder
  to diagnose.
- CLIs like adb and simctl are stable and cheap to wrap.
- We control the release cadence and capability boundaries.
- Concretely, as of mobile-mcp's 2026-10-01 main branch, its Android element list is a flat list of nodes that
  have text, a description, a hint or a resource-id, without clickable or enabled state, and with no stable refs
  across dumps, no diffs and no wait-for-idle. Those are exactly the parts we need to own.

## Rejected alternatives
- **Depend on mobile-mcp:** faster start, but the core experience would depend on another project.
- **Pluggable, support both:** the most work, and the abstraction would likely be wrong before the core experience
  has taken shape.

## Consequences
- We own compatibility across Android versions (covered by fixtures from multiple API levels).
- The `Driver` trait must be neutral enough to leave room for iOS and the helper backend.
