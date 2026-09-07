# Course Order Assistant

Rust CLI for NTUST course-choice analysis and vacancy monitoring with ntfy notifications.
The `rust` branch is the source of upstream PR #226. Preserve both analysis and monitoring.

## Dependencies and verification

- Keep upstream dependency updates and the monitor-specific rand and chrono dependencies.
- reqwest 0.13 requires explicit `query`; preserve upstream `native-tls-vendored` for the Linux musl build.
- Commit Cargo.lock and verify with `cargo test --locked`, `cargo build --release --locked`, and a menu/exit smoke test.
- Dependency regression tests are in src/dependency_tests.rs; they do not send notifications or require student data.
- Real monitoring needs school API access and an explicitly approved notification destination; offline tests do not prove delivery.

## Runtime contracts

- One stdin reader serves menus and monitoring. EOF exits cleanly; Q cancels polling, retries, notifications and countdown waits.
- HTTP clients have a 10-second connect timeout and 20-second request timeout.
- Count vacancy notifications only after successful delivery; failed attempts remain eligible on the next polling cycle.
- Store a random 256-bit ntfy topic in course_assistant_config.json beside the executable. Legacy configs migrate on first monitoring start and require re-subscription; student IDs and hardware fingerprints are no longer used.
- Topic names are not encryption or authentication. Treat the config and topic as private; never print them in test logs or commit them.
- Missing configs use defaults; unreadable, invalid or unwritable configs produce errors instead of silent resets.
- Configuration writes use tempfile and replace the destination only after the complete new file is written; Unix files are owner-only.

## CI and release caution

CI builds macOS, Windows and Linux musl. A push to `rust` also replaces existing assets on the latest fork release.
Confirm that release side effect before pushing. Never commit student configuration, credentials, runtime files or target outputs.
