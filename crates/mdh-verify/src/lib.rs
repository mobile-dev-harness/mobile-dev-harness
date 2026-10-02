//! Verify: the verification engine every check runs in (ADR-0009).
//!
//! Scope (functional design F6, F7; milestone M4): flows (recorded from sessions, replayed
//! deterministically), the `Check` interface that check kinds implement, one verdict per run that
//! collects their findings with evidence (screenshots, tree excerpts, logs, numbers), the baseline
//! store, and reports (JUnit). Functional checks — assertions on screens and logs — are built in;
//! UI consistency (`mdh-visual`) and performance (`mdh-perf`) plug in as further check kinds.
