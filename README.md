# d2b-wlterm

`d2b-wlterm` is the Home Manager and presentation companion for persistent d2b
terminal sessions. Version 2.0 prepares the client boundary for d2b's canonical
client, contract, and session crates without copying their protocols.

The repository currently provides its presentation reducer, Waybar output,
Quickshell control center, deterministic review renderer, CLI package, and Home
Manager module. Live target discovery, session setup, persistent-shell streams,
and Wayland control fail closed until their canonical services are available.
There is no legacy public-JSON or direct-compositor fallback.

## Source ownership

The `d2b-client-toolkit` input is pinned exactly. Its canonical d2b source is
revision `4018d9c9652bd826c2e6a9abccdcdcafb832d944`, fingerprint
`c2c99bdd77ba66948fce81161dcc3efde608eefefb96f28fa934c9f58d96d838`,
and inventory digest
`2aaef697cc53abc8757a3593352cd5bd1d3f0d3f2031c6a2967f92afa5e74d97`.
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
      url = "github:vicondoa/d2b-toolkit/800c2878533f600d8f085b3d2aafcddb970232b2";
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
