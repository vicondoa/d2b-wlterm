# d2b-wlterm

`d2b-wlterm` is the planned Wayland terminal launcher surface for d2b. This
repository currently contains core VM/session models, a d2b toolkit adapter
boundary, Waybar output helpers, UI state concepts, a CLI binary, and a Home
Manager module.

## Current status

Implemented:

- bounded friendly random-name allocation for terminal sessions;
- core reducer and action planner for VM/session state;
- offline VM guards that disable shell list/create/open actions;
- Stop confirmation, already-attached Open, and async error-display models;
- `d2b-wlterm` CLI with public-socket shell list, open/create, and
  confirmed stop commands;
- `homeManagerModules.default` with package install, `config.toml` rendering,
  Waybar integration, and a Quickshell control-center state surface;
- safe UI rendering for shell labels, manual create-name prompts,
  already-attached fallbacks, and async errors with bounded digest/correlation
  details;
- a local `d2b-toolkit`/`d2b-client` boundary for public daemon shell actions;
- realm-aware VM discovery metadata from `d2b list --json`, preserving
  d2b-provided canonical targets and falling back to `<vm>.local.d2b` for local
  VMs while the current shell public-socket verbs still address the local VM id.

The d2b integration crate uses only the public daemon socket. Stop dispatches a
shell kill only after confirmation, and closing an attached terminal view sends a
disconnect request rather than killing the shell.

## Development

```bash
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
nix flake check
```

## Flake inputs

Use one `nixpkgs` input across d2b, the toolkit, and this launcher. If you also
use the WeezTerm flake, make it follow the same toolkit input:

```nix
{ inputs, pkgs, ... }:
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    d2b = {
      url = "github:vicondoa/d2b";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    d2b-toolkit = {
      url = "github:vicondoa/d2b-toolkit";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    d2b-wlterm = {
      url = "github:vicondoa/d2b-wlterm";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.d2b-toolkit.follows = "d2b-toolkit";
    };

    weezterm = {
      url = "github:vicondoa/weezterm";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.d2b-toolkit.follows = "d2b-toolkit";
    };
  };
}
```

The flake exports `packages.${system}.default`,
`homeManagerModules.default`, and a `checks.${system}.home-manager-module`
evaluation check for the rendered Home Manager config and Waybar snippet.

## Home Manager

```nix
{
  imports = [ inputs.d2b-wlterm.homeManagerModules.default ];

  programs.d2b-wlterm = {
    enable = true;
    publicSocketPath = "$XDG_RUNTIME_DIR/d2b/public.sock";
    weztermCommand = [
      "${inputs.weezterm.packages.${pkgs.stdenv.hostPlatform.system}.default}/bin/weezterm"
      "start"
      "--"
    ];
    waybar.enable = true;
    quickshell.enable = true;
  };
}
```
