# d2b-wlterm agent notes

This repository is the d2b Wayland terminal launcher companion. Keep the public
surface small and predictable: Rust crates under `crates/`, Nix/Home Manager
wiring under `nix/`, and user-facing docs under `docs/`.

## Validate

Prefer focused checks while iterating:

```bash
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
nix flake check
```

## Integration notes

- Depend on `d2b-toolkit` through a local path when that sibling repository is
  available, but keep this skeleton buildable without it.
- Do not make the Home Manager module depend on host-private d2b state. It should
  render user config and invoke the installed CLI only.
- Keep Stop confirmation, already-attached Open, and async errors explicit in the
  model layer so UI frontends can present safe choices.
