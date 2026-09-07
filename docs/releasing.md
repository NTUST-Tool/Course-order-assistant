# Publishing a release

Code review and publishing are separate. The CI workflow runs tests, builds executables, smoke-tests them, and uploads artifacts. It does not write releases or tags, and its token has read-only repository permissions. A push or pull request cannot replace existing downloads.

To publish after explicit approval:

1. Choose an unused version tag and the exact source commit. Use the repository you intend to publish to; publishing a fork release does not publish an upstream release or merge its pull request.
2. Require a successful CI run for that commit on Windows, Linux musl and macOS. If needed, run CI manually using `workflow_dispatch`.
3. Download the run's artifacts. Package the Windows executable, Linux executable ZIP and macOS executable ZIP with names matching their verified platforms. Do not label a single-architecture macOS build as Universal or Intel.
4. Generate SHA-256 checksums. Include relevant changes, migration instructions and verification limitations in release notes.
5. Create a **new draft release** for the exact commit, attach the verified files, and verify the tag, asset names and checksums. Download the draft assets again to check their integrity before publication.
6. Publish the draft. Confirm the public release contains every expected asset and that previous releases retain their original assets and tags.

Do not delete existing assets, overwrite an existing release, or move an old tag. A new release need not provide every platform that an older release supported; retain older downloads and state the new release's platform coverage clearly.

Keep release archives, binaries, configuration files and signing credentials out of Git. Mobile notification delivery is not established by a server-side ntfy subscription test.
