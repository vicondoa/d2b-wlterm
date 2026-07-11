# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
- d2b-toolkit is pinned to version 0.2.0 at revision
  release tag `v0.2.0` (locked to
  `fde6af8b842718e7150f5056d4eba73093d4ad77`).
- All workspace crates and flake package outputs are version 0.2.0.

### Security

- Unsafe-local shell operations require negotiated `unsafe-local-shell-v1` and
  never fall back to a host shell, SSH, helper socket, broker, or private state.
- Terminal windows fail closed when proxy readiness fails; there is no direct
  compositor fallback.
- Errors and diagnostics omit opaque handles, terminal bytes, argv, environment,
  cwd, and private paths.
