# Release builds

`Build release artifacts` creates native `semblock` archives for the supported
PoC release platforms.

## Triggering the workflow

The workflow runs in two modes:

- `workflow_dispatch` builds and retains GitHub Actions artifacts for 30 days;
- pushing a tag matching `v*` builds the same artifacts and creates or updates
  the corresponding GitHub Release.

A release tag must exactly match the Cargo package version, for example
`Cargo.toml` version `0.1.0` must use tag `v0.1.0`.

## Artifact matrix

| Artifact target | Runner/build environment | Runtime baseline |
| --- | --- | --- |
| `x86_64-pc-windows-msvc` | Windows Server 2022/MSVC | Windows 10 x64 or newer |
| `x86_64-unknown-linux-gnu` | Rocky Linux 9 container | GLIBC 2.34 or newer |
| `x86_64-apple-darwin` | macOS 15 Intel | macOS 14 or newer |
| `aarch64-apple-darwin` | macOS 15 ARM64 | macOS 14 or newer |

The Linux build is produced inside a Rocky Linux 9 container rather than on the
Ubuntu runner directly. The workflow inspects the finished ELF binary and fails
if it imports a GLIBC symbol newer than `GLIBC_2.34`. It then downloads and
executes the packaged archive in clean Rocky Linux 9, Debian 12, and Ubuntu
24.04 containers. This validates both the runtime baseline and the archive
contents against every declared Linux distribution family.

macOS artifacts are separate native Intel and Apple Silicon archives rather
than a universal binary. This keeps each artifact smaller and ensures that both
architectures compile and smoke-test independently.

## Archive contents

Every platform archive has a single top-level directory containing:

- `semblock` or `semblock.exe`;
- `README.md`;
- `LICENSE`;
- `THIRD_PARTY_NOTICES.md`.

`.github/scripts/package_release.py` normalizes archive entry ordering,
permissions, ownership metadata, and timestamps using `SOURCE_DATE_EPOCH`.
The combined release bundle contains all platform archives and `SHA256SUMS`.

## Deliberate limits

- Linux ARM64 and Windows ARM64 are not currently release targets.
- Binaries are not code-signed or notarized.
- macOS users may need to approve an unsigned binary through system security
  controls until signing and notarization are configured.
- The workflow verifies runtime compatibility and performs a CLI smoke test, but
  the complete test suite remains the responsibility of the regular `CI`
  workflow.
