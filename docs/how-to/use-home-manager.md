# Use d2b-wlterm with Home Manager

Pin the canonical client distribution and import the module:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    d2b-client-toolkit = {
      url = "github:vicondoa/d2b-toolkit/3d6b75d47c8df66c1722ea324d64334a127d43ec";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    d2b-wlterm = {
      url = "github:vicondoa/d2b-wlterm";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.d2b-client-toolkit.follows = "d2b-client-toolkit";
    };

    home-manager = {
      url = "github:nix-community/home-manager";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { d2b-wlterm, home-manager, ... }: {
    homeConfigurations.alice = home-manager.lib.homeManagerConfiguration {
      modules = [
        d2b-wlterm.homeManagerModules.default
        {
          programs.d2b-wlterm.enable = true;
          programs.d2b-wlterm.defaultOpenBehavior = "focus-existing";
          programs.d2b-wlterm.waybar.enable = true;
          programs.d2b-wlterm.quickshell.enable = true;
        }
      ];
    };
  };
}
```

The module installs the package, renders `d2b-wlterm/config.toml`, optionally
injects a Waybar module, and writes a Quickshell presentation descriptor. It
owns only user configuration. The descriptor intentionally exposes no live
shell or Wayland actions until the canonical services are available.

Run `nix flake check` after changing the module wiring.
