# Contributing

Thanks for your interest! The project is early, so design discussion in issues is very welcome
before large PRs.

## Development

```sh
cargo build
cargo test                 # unit tests, no device required
cargo fmt --all
cargo clippy --all-targets -- -D warnings
```

Requirements: stable Rust (MSRV 1.85), and for manual testing an Android SDK with platform-tools
and at least one AVD. Run `cargo run -- doctor` to check your setup.

## Conventions

- **Parsers are pure and fixture-tested.** Anything that parses tool output (`adb`, `uiautomator`,
  `logcat`, Gradle) is a plain function over `&str`, tested with real captured output.
- **Device-dependent tests** are gated behind `MDH_E2E=1` and must not run by default.
- **Agent-facing output is a product surface.** Keep it compact, stable and actionable; every
  `--json` shape change is a breaking change.
- Crate layout and dependency direction are described in [docs/DESIGN.md](docs/DESIGN.md).
