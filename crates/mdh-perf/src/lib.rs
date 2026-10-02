//! Perf: measures how fast and lean the app is, and catches regressions.
//!
//! Scope (functional design F11; milestone M6): cold/warm startup over repeated runs, frame timing
//! and jank (`dumpsys gfxinfo`), memory (`dumpsys meminfo`) including growth across repeated flows,
//! CPU sampling, budgets in config and comparison against stored baselines. Emulator numbers are
//! noisy, so results are relative to a baseline on the same device and report their variance.
