## What and why

<!-- What the change does, and the problem it solves (link the issue). -->

## How it was verified

<!-- Tests added or changed; for behavior on a device, what you ran (`mdh verify …`, `mdh flow run …`) and the
verdict. -->

## Checklist

- [ ] `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass
- [ ] Agent-facing output (text or `--json`) changed only on purpose, and the CHANGELOG says so
- [ ] Docs updated; `README.zh-CN.md` kept in sync with `README.md`
