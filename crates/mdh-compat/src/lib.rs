//! Compat: the compatibility matrix (ADR-0009). Not a check kind: it runs flows and their checks
//! (functional, UI consistency, performance) on every cell and aggregates the cells' verdicts.
//!
//! Scope (functional design F12; milestone M7): matrix axes (Android version, form factor, size,
//! orientation, locale/RTL, font scale, dark mode, display size, vendor), a device pool (local
//! emulators, physical devices, later cloud providers), per-device configuration without new AVDs
//! where possible, parallel scheduling, and a deduplicated matrix report.
