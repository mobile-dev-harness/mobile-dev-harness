//! Project: understands the app's source project so the other domains can build and run it.
//!
//! Scope (functional design F2; milestone M2): probe Gradle projects with an init script (modules,
//! variants, application id, artifacts), build incrementally, and parse compiler, resource, manifest
//! and Gradle failures into structured diagnostics. Later adapters: React Native, Expo, Flutter,
//! Xcode.
