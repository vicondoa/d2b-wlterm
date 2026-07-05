# Use d2b-wlterm with Home Manager

Add the flake input and import the module:

```nix
{
  inputs.d2b-wlterm.url = "github:vicondoa/d2b-wlterm";

  outputs = { d2b-wlterm, ... }: {
    homeConfigurations.alice = home-manager.lib.homeManagerConfiguration {
      modules = [
        d2b-wlterm.homeManagerModules.default
        {
          programs.d2b-wlterm.enable = true;
          programs.d2b-wlterm.defaultOpenBehavior = "focus-existing";
          programs.d2b-wlterm.waybar.enable = true;
        }
      ];
    };
  };
}
```

The module installs the package, renders `d2b-wlterm/config.toml`, and can render
a Waybar module snippet at `d2b-wlterm/waybar-module.json`. Use
`defaultOpenBehavior = "prompt"` or `"force-open"` if focusing an existing
attached terminal should not be the default. Use `asyncErrorDisplay = "inline"`
when a frontend should render delayed d2b client failures inline instead of as a
notification or Waybar state.
