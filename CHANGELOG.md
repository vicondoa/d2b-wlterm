# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Transient Quickshell control-center focus lifecycle with a per-process pin,
  keyboard-accessible pin control, and chrome dragging within the compositor's
  usable output area.
- Deterministic `render-sample <output.png>` review capture using mocked
  workloads and the production QML tree.

### Changed

- Switched the `d2b-client-toolkit` dependency from an adjacent Cargo path
  dependency to an exact pinned git revision, removed the CI workaround that
  cloned a sibling checkout, and dropped the flake package-source path
  substitution in favor of Nix's own fixed-output vendoring of that same
  revision; the canonical d2b source revision and fingerprint are unchanged.
- Added hermetic `cargo fmt --check` and `cargo clippy -D warnings` checks to
  `nix flake check` so formatting and lint regressions are caught without a
  developer checkout.
- Restored the Home Manager release check's assertions on a non-empty
  generated `wayland_proxy_command` and its default `d2b-wayland-proxy`
  command.
- Repinned the client toolkit checkout, flake input, and release-metadata
  assertions to the toolkit's final W9 PR head (a CI/changelog-only revision);
  the canonical d2b source revision and fingerprint are unchanged.
- Aligned CI and Home Manager guidance with the frozen W9 client toolkit
  revision used by Cargo and the flake.
- Pinned the frozen W6 client toolkit and migrated workload discovery plus
  persistent-shell list, detach, and kill to authenticated ComponentSession
  clients and canonical terminal selections.
- Kept interactive shell stream launch fail-closed pending its canonical
  desktop route; no legacy public JSON, SSH, helper socket, or compositor
  fallback was added.
- Prepared the 2.0 client cutover by pinning the canonical
  `d2b-client-toolkit` distribution and its exact d2b source revision and
  fingerprint, including the final source inventory digest.
- Audited wlterm's reducer, UI, and Waybar state as repository-local,
  output-only presentation models; configuration remains the only
  deserializable local format.
- Home Manager remains the owner of the user package and presentation
  configuration, while live session and Wayland actions are withheld until
  their canonical services are available.
- Realm accents now use rounded outer frames with neutral inset surfaces.

### Fixed

- Fixed the daemon connection's non-blocking `connect(2)` to correctly wait
  out an in-progress handshake (`EINPROGRESS`) and retry past a transiently
  full listen backlog (`EAGAIN`) within a bounded deadline, instead of failing
  the connection on the first transient error; added a concurrent
  backlog-contention regression test.

### Removed

- Removed the copied toolkit 0.2 workload fixtures, their protocol-conformance
  tests, the native public-JSON integration test, and the legacy public-socket
  adapter.
- Removed legacy Wayland proxy readiness and terminal-stream setup. Target
  discovery, persistent-shell operations, streams, and Wayland control now
  fail closed instead of guessing pending APIs.

### Security

- Canonical target, session, service, stream, and transport contracts remain
  owned by d2b; wlterm no longer deserializes presentation state as an
  alternate wire format.

## [0.2.0] - 2026-07-11

### Added

- Public workload inventory discovery through d2b-toolkit 0.2.0, restricted to
  workloads advertising `persistent-shell` and a shell launcher item.
- Canonical target support for local VMs, first-class local VMs without legacy
  names, provider-managed targets, and `unsafe-local` targets.
- Provider kind, isolation posture, session persistence, availability, and
  typed remediation in realm-grouped control-center state.
- Explicit `unsafe-local` no-isolation warnings and helper/update remediation.
- Target-aware shell create, list, open, detach, and confirmed stop dispatch.
- Typed d2b-wayland-proxy first-client readiness for GUI terminal windows.
- Home Manager configuration, Waybar integration, and Quickshell control-center
  actions for the target-aware launcher.

### Changed

- Discovery now uses only the negotiated d2bd public socket; it no longer
  invokes CLI subprocesses or reads host-private artifacts.
- Terminal models and actions use canonical workload targets while retaining
  legacy VM JSON fields and aliases for compatibility.
- d2b-toolkit is pinned to release tag `v0.2.0` (locked to
  `fde6af8b842718e7150f5056d4eba73093d4ad77`).
- All workspace crates and flake package outputs are version 0.2.0.

### Security

- Unsafe-local shell operations require negotiated `unsafe-local-shell-v1` and
  never fall back to a host shell, SSH, helper socket, broker, or private state.
- Terminal windows fail closed when proxy readiness fails; there is no direct
  compositor fallback.
- Public-socket operations use a non-blocking reactor transport with bounded
  operation deadlines, reusable packet buffers, and interrupted-syscall retries;
  connect polling preserves its absolute deadline across signals, and proxy
  readiness frames are size- and deadline-bound.
- Errors and diagnostics omit opaque handles, terminal bytes, argv, environment,
  cwd, private paths, and raw target identifiers.
