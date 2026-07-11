# d2b-wlterm

`d2b-wlterm` 0.2.0 is the Wayland terminal launcher companion for persistent
d2b workload shells. It provides a Rust model/client boundary, Waybar output,
a Quickshell control center, and a Home Manager module.

## Behavior

- Discovers workloads directly from the negotiated d2bd public socket through
  d2b-toolkit 0.2.0.
- Shows only workloads advertising both `persistent-shell` and a shell launcher
  item.
- Addresses shell operations by canonical target, such as
  `builder.dev.d2b` or `tools.host.d2b`; legacy local VM names remain accepted.
- Supports first-class local VMs without `legacyVmName`.
- Groups workloads by realm while preserving realm accent rails.
- Shows provider kind, isolation, session persistence, availability, and typed
  remediation. `unsafe-local` is labeled **NO ISOLATION**.
- Requires `unsafe-local-shell-v1` before exposing unsafe-local shell actions.
- Sends create/list/open/detach/confirmed-stop through d2b-toolkit shell
  methods. It never discovers through the CLI or reads host-private state.
- Opens WezTerm only through `d2b-wayland-proxy` and waits for typed
  first-client readiness. Proxy failure has no direct-compositor fallback.

Stop remains explicitly confirmed, and an attached shell keeps the existing
focus/prompt/force-open behavior. Closing a terminal attachment detaches it; it
does not kill the persistent session.

## Development

Use the shared target directory for local validation:

```bash
export CARGO_TARGET_DIR=/home/paydro/.cache/d2b-wlterm-target
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
nix flake check
```

## Flake inputs

This release pins d2b-toolkit 0.2.0 at
`fde6af8b842718e7150f5056d4eba73093d4ad77`. Consumers should keep one toolkit
and nixpkgs revision across desktop companions:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    d2b = {
      url = "github:vicondoa/d2b";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    d2b-toolkit = {
      url = "github:vicondoa/d2b-toolkit/fde6af8b842718e7150f5056d4eba73093d4ad77";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    d2b-wlterm = {
      url = "github:vicondoa/d2b-wlterm/v0.2.0";
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

The flake exports `packages.${system}.default`, `apps.${system}.default`,
`homeManagerModules.default`, and package/Home Manager checks.

## Home Manager

```nix
{
  imports = [ inputs.d2b-wlterm.homeManagerModules.default ];

  programs.d2b-wlterm = {
    enable = true;
    publicSocketPath = "/run/d2b/public.sock";
    weztermCommand = [
      "${inputs.weezterm.packages.${pkgs.stdenv.hostPlatform.system}.default}/bin/wezterm"
      "start"
      "--"
    ];
    waylandProxyCommand = [ "d2b-wayland-proxy" ];
    waybar.enable = true;
    quickshell.enable = true;
  };
}
```

The module writes only user configuration. Inventory and shell operations use
the public daemon socket; the launcher never reads bundle artifacts, root state,
the broker socket, or the private unsafe-local helper transport.
