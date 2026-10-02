//! Visual: UI consistency checks, a check kind of the verification engine (ADR-0009).
//!
//! Scope (functional design F13; milestone M5): structural and pixel comparison against baselines
//! with masks for dynamic regions, comparison against design mocks (Figma), cross-configuration
//! layout problems (truncation, overlap, clipping), and rule checks on the tree and pixels (touch
//! target size, missing labels, contrast, design tokens). Implements `mdh_verify::Check`; findings
//! land in the run's verdict.
