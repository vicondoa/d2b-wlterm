# Use d2b-wlterm with Home Manager

Add the flake input and import the module:

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
      url = "github:vicondoa/d2b-wlterm";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.d2b-toolkit.follows = "d2b-toolkit";
    };

    weezterm = {
      url = "github:vicondoa/weezterm";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.d2b-toolkit.follows = "d2b-toolkit";
    };

    home-manager = {
      url = "github:nix-community/home-manager";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { d2b-wlterm, home-manager, weezterm, ... }: {
    homeConfigurations.alice = home-manager.lib.homeManagerConfiguration {
      modules = [
        d2b-wlterm.homeManagerModules.default
        ({ pkgs, ... }: {
          programs.d2b-wlterm.enable = true;
          programs.d2b-wlterm.weztermCommand = [
            "${weezterm.packages.${pkgs.stdenv.hostPlatform.system}.default}/bin/weezterm"
            "start"
            "--"
          ];
          programs.d2b-wlterm.defaultOpenBehavior = "focus-existing";
          programs.d2b-wlterm.waybar.enable = true;
          programs.d2b-wlterm.quickshell.enable = true;
        })
      ];
    };
  };
}
```

The module installs the package, renders `d2b-wlterm/config.toml`, can render a
Waybar module snippet at `d2b-wlterm/waybar-module.json`, and injects the custom
Waybar module when Home Manager also manages Waybar. Enable
`quickshell.enable` to render the control-center surface description at
`d2b-wlterm/quickshell-control-center.json`.

The toolkit pin is release 0.2.0. Keep WezTerm and other desktop companions
following the same toolkit input. Ensure `d2b-wayland-proxy` is on the
user-session `PATH`, or set `waylandProxyCommand` to its absolute package path.
A missing or unready proxy fails terminal open without a direct Wayland
fallback.

Use `defaultOpenBehavior = "prompt"` or `"force-open"` if focusing an existing
attached terminal should not be the default. Use `asyncErrorDisplay = "inline"`
when a frontend should render delayed d2b client failures inline instead of as a
notification or Waybar state.

Run `nix flake check` after changing the module wiring. The upstream flake's
`checks.<system>.home-manager-module` check evaluates the rendered TOML config
and Waybar module shape without starting d2b.
