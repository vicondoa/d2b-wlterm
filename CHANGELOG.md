# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Initial Rust/Nix workspace skeleton with core, d2b, Waybar, UI, and CLI crates.
- Home Manager module scaffold for package installation, config rendering, and Waybar integration.
- Deterministic friendly-name generator and model stubs for planned terminal lifecycle behavior.
- Core VM/session reducer and action planner with offline VM guards, Stop confirmation,
  already-attached Open fallbacks, async error state, and bounded friendly shell-name allocation.
- Local d2b-toolkit DTO integration boundary for planned shell actions.
- Public d2b client execution boundary for shell list, open/create attach,
  disconnect-only close, and confirmed Stop-to-kill actions with redacted
  trace/correlation errors.
