# d2b-wlterm

`d2b-wlterm` is the Home Manager and presentation companion for persistent d2b
terminal sessions. Version 2.0 uses d2b's canonical authenticated client,
contract, session, and terminal APIs without copying their protocols.

The repository currently provides its presentation reducer, Waybar output,
Quickshell control center, deterministic review renderer, CLI package, and Home
Manager module. Authenticated discovery and shell management use the frozen
service facade. Interactive desktop stream routing remains fail-closed until
its canonical desktop route is available. There is no legacy public-JSON, SSH,
helper-socket, or direct-compositor fallback.

## Source ownership

The `d2b-client-toolkit` input is pinned exactly. Its canonical d2b source is
revision `9dc902243cdd7aba7ef269988b96f0aae6e037da`, fingerprint
`5a20cef3a64281df819eeb76bdfe385999755479b467b559653011582fb9c043`,
and inventory digest
`35c33c2e23e1b9f03b5abc3bbca2d3320e38c42dfc7aceb7e3476d28210cde8c`.
`wlterm-core`, `wlterm-ui`, and `wlterm-waybar` own only repository-local
configuration and presentation state.

See [Presentation model ownership](docs/reference/presentation-model.md).

## Development

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
nix flake check
```

Generate a deterministic review image inside a Niri/Wayland session:

```bash
cargo run -p wlterm-cli -- render-sample ./wlterm-control-center.png
```

## Flake and Home Manager

Until the deferred GitHub repository rename completes, the input name is the
new `d2b-client-toolkit` name while its URL uses the existing toolkit
repository:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    d2b-client-toolkit = {
      url = "github:vicondoa/d2b-toolkit/926de54e7320599c373524a10b65aaf13b6ff422";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    d2b-wlterm = {
      url = "github:vicondoa/d2b-wlterm";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.d2b-client-toolkit.follows = "d2b-client-toolkit";
    };
  };

  outputs = { d2b-wlterm, ... }: {
    homeModules = [
      d2b-wlterm.homeManagerModules.default
      {
        programs.d2b-wlterm = {
          enable = true;
          waybar.enable = true;
          quickshell.enable = true;
        };
      }
    ];
  };
}
```

The module owns the user package and files under
`$XDG_CONFIG_HOME/d2b-wlterm`. It does not read host-private d2b state or own
daemon, session, helper, or Wayland services.
