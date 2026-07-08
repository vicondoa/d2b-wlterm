# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Documented flake input alignment for d2b, d2b-toolkit, d2b-wlterm, and
  WeezTerm, including Home Manager wiring and the flake check that evaluates the
  rendered module output.
- Initial Rust/Nix workspace skeleton with core, d2b, Waybar, UI, and CLI crates.
- Home Manager module scaffold for package installation, config rendering, and Waybar integration.
- Deterministic friendly-name generator and model stubs for planned terminal lifecycle behavior.
- Core VM/session reducer and action planner with offline VM guards, Stop confirmation,
  already-attached Open fallbacks, async error state, and bounded friendly shell-name allocation.
- Local d2b-toolkit DTO integration boundary for planned shell actions.
- Public d2b client execution boundary for shell list, open/create attach,
  disconnect-only close, and confirmed Stop-to-kill actions with redacted
  trace/correlation errors.
- Waybar status JSON, control-center state JSON, and Home Manager Waybar
  injection with sanitized labels, active-shell counts, disabled VM state,
  already-attached fallbacks, manual create-name prompts, and safe async-error
  rendering.
- Native CLI integration coverage that drives `d2b-wlterm list` over a real
  AF_UNIX public-socket frame exchange instead of a fake in-memory transport.
- Quickshell control center that opens from Waybar, shows shell-capable VMs and
  their active terminals, and dispatches create/open/confirmed-stop actions.
- WeezTerm launches are wrapped with `d2b-wayland-proxy --host-terminal` so
  terminal windows receive VM identity rails and d2b clipboard policy.
- Added explicit shell detach handling and launch-time WeezTerm close-confirmation
  suppression so closing a terminal window detaches from the persistent d2b shell
  without prompting to kill tabs.
- Added realm-aware VM discovery metadata: `d2b-wlterm` now prefers
  `d2b list --json`, preserves d2b-provided canonical realm targets, and falls
  back to `<vm>.local.d2b` for local VM labels while shell operations continue
  using the current public socket VM id.

### Changed

- Nix flake packaging consumes `d2b-toolkit` from a GitHub flake input with
  `nixpkgs` following the caller, avoiding developer-local absolute paths.
- VM cards in the control center use `/etc/d2b/ui-colors.json` so the app
  accents each VM with the same d2b color used by the Wayland proxy rail.
- The control-center popup now expands to content up to a larger screen-bound
  max height and uses d2b-wlcontrol-matched icon sizing, hover behavior, and
  borderless white/gray action icons.
