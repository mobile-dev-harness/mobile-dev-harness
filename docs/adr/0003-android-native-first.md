# ADR-0003: Native Android projects first

- Status: Accepted (2026-10-02)

## Decision
The first release fully supports only native Android Gradle projects. The architecture reserves interfaces for
iOS, React Native, Expo and Flutter (`Driver`, `ProjectAdapter`) without implementing them ahead of time.

## Rationale
- adb is feature-complete (input, UI dump, logcat, permissions, snapshots), so implementation is fastest.
- Android emulators run on Linux CI (KVM), keeping E2E tests cheap.
- The Android side of RN, Expo and Flutter is a Gradle project, so their adapters can build on solid native support.

## Consequences
- iOS users wait for a later release; the hard parts of iOS (simulator UI interaction needs extra tooling,
  xcodebuild diagnostics) are deferred.
- The neutral abstractions lack a second platform to validate them. Mitigated by checking each trait method
  against iOS capabilities during design (architecture §3.2).
