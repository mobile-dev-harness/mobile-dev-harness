//! Compat: runs the same checks across devices and configurations and reports the matrix.
//!
//! Scope (functional design F12; milestone M7): matrix axes (Android version, form factor, size,
//! orientation, locale/RTL, font scale, dark mode, display size, vendor), a device pool (local
//! emulators, physical devices, later cloud providers), per-device configuration without new AVDs
//! where possible, parallel runs of flows plus visual and perf checks, and a deduplicated report.
