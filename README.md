# d2b-wlterm

`d2b-wlterm` is the planned Wayland terminal launcher surface for d2b. This
repository currently contains core session models, d2b adapter stubs, Waybar
output helpers, UI state concepts, a CLI binary, and a Home Manager module
scaffold.

## Current status

Implemented as scaffolding only:

- friendly random-name generation for terminal sessions;
- Stop confirmation, already-attached Open, and async error-display model stubs;
- `d2b-wlterm` CLI with small smoke-test commands;
- `homeManagerModules.default` with package install, `config.toml` rendering, and
  Waybar module-file rendering;
- docs and CI placeholders for future feature work.

The d2b integration crate intentionally uses a local stub until
`github.com/vicondoa/d2b-toolkit` is available. See the TODO path dependency in
`Cargo.toml`.

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
