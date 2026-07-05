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
- a local `d2b-toolkit`/`d2b-client` boundary for public daemon shell actions.

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

## Home Manager

```nix
{
  imports = [ inputs.d2b-wlterm.homeManagerModules.default ];

  programs.d2b-wlterm = {
    enable = true;
    publicSocketPath = "$XDG_RUNTIME_DIR/d2b/public.sock";
    weztermCommand = [ "wezterm" "start" "--" ];
    waybar.enable = true;
    quickshell.enable = true;
  };
}
```
