# Course Order Assistant

Rust CLI for NTUST course-choice analysis and vacancy monitoring with ntfy notifications.
The `rust` branch is the source of upstream PR #226. Preserve both analysis and monitoring.

## Dependencies and verification

- Keep upstream dependency updates and the monitor-specific rand, chrono, sha2 and machineid-rs dependencies.
- reqwest 0.13 requires explicit `query`; preserve upstream `native-tls-vendored` for the Linux musl build.
- Commit Cargo.lock and verify with `cargo test --locked`, `cargo build --release --locked`, and a menu/exit smoke test.
- Dependency regression tests are in src/dependency_tests.rs; they do not send notifications or require student data.
- Real monitoring needs school API access and an explicitly approved notification destination; offline tests do not prove delivery.

## CI and release caution

CI builds macOS, Windows and Linux musl. A push to `rust` also replaces existing assets on the latest fork release.
Confirm that release side effect before pushing. Never commit student configuration, credentials, runtime files or target outputs.
