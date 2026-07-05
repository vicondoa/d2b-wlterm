# d2b-wlterm

`d2b-wlterm` is the planned Wayland terminal launcher surface for d2b. This repository currently contains core VM/session models, a d2b toolkit
adapter boundary, Waybar output helpers, UI state concepts, a CLI binary, and a
Home Manager module scaffold.

## Current status

Implemented:

- bounded friendly random-name allocation for terminal sessions;
- core reducer and action planner for VM/session state;
- offline VM guards that disable shell list/create/open actions;
- Stop confirmation, already-attached Open, and async error-display models;
- `d2b-wlterm` CLI with small smoke-test commands;
- `homeManagerModules.default` with package install, `config.toml` rendering, and
  Waybar module-file rendering;
- a local `d2b-toolkit` DTO boundary for future public-socket transport work.

The d2b integration crate intentionally stops at typed action-to-DTO mapping.
Runtime socket transport remains behind that boundary.

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
    weztermCommand = [ "weezterm" "start" "--" ];
    waybar.enable = true;
  };
}
```
