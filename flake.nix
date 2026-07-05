{
  description = "Wayland terminal launcher companion for d2b";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    d2b-toolkit = {
      url = "github:vicondoa/d2b-toolkit";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, d2b-toolkit }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
          toolkitSource = d2b-toolkit.packages.${system}.default;
        in
        {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "d2b-wlterm";
            version = "0.1.0-dev";
            src = pkgs.lib.cleanSource ./.;
            postPatch = ''
              substituteInPlace Cargo.toml \
                --replace-fail "../d2b-toolkit/crates/d2b-toolkit-core" \
                  "${toolkitSource}/share/d2b-toolkit/crates/d2b-toolkit-core" \
                --replace-fail "../d2b-toolkit/crates/d2b-client" \
                  "${toolkitSource}/share/d2b-toolkit/crates/d2b-client"
            '';
            cargoLock.lockFile = ./Cargo.lock;
            cargoBuildFlags = [ "-p" "wlterm-cli" ];
            cargoTestFlags = [ "--workspace" ];
            meta = {
              description = "Wayland terminal launcher companion for d2b";
              homepage = "https://github.com/vicondoa/d2b-wlterm";
              license = pkgs.lib.licenses.asl20;
              mainProgram = "d2b-wlterm";
            };
          };
        });

      checks = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
          hmEval = pkgs.lib.evalModules {
            specialArgs = { inherit pkgs; };
            modules = [
              ({ lib, ... }: {
                options.assertions = lib.mkOption {
                  type = lib.types.listOf lib.types.anything;
                  default = [ ];
                };
                options.home.packages = lib.mkOption {
                  type = lib.types.listOf lib.types.package;
                  default = [ ];
                };
                options.xdg.configFile = lib.mkOption {
                  type = lib.types.attrsOf lib.types.anything;
                  default = { };
                };
                options.programs.waybar.enable = lib.mkOption {
                  type = lib.types.bool;
                  default = false;
                };
                options.programs.waybar.settings = lib.mkOption {
                  type = lib.types.attrsOf (lib.types.submodule {
                    freeformType = lib.types.attrsOf lib.types.anything;
                    options."modules-right" = lib.mkOption {
                      type = lib.types.listOf lib.types.str;
                      default = [ ];
                    };
                  });
                  default = { };
                };
              })
              (import ./nix/home-manager.nix { inherit self; })
              {
                programs.d2b-wlterm.enable = true;
                programs.d2b-wlterm.defaultOpenBehavior = "force-open";
                programs.d2b-wlterm.weztermCommand = [ "weezterm" "start" "--" ];
                programs.d2b-wlterm.waybar.enable = true;
                programs.d2b-wlterm.quickshell.enable = true;
                programs.waybar.enable = true;
                programs.waybar.settings.mainBar.modules-right = [ "clock" ];
              }
            ];
          };
        in {
          package = self.packages.${system}.default;
          home-manager-module = pkgs.runCommand "d2b-wlterm-home-manager-module" { } ''
            test -n "${hmEval.config.xdg.configFile."d2b-wlterm/config.toml".source}"
            grep -q 'public_socket_path = "/run/d2b/public.sock"' "${hmEval.config.xdg.configFile."d2b-wlterm/config.toml".source}"
            grep -q 'wezterm_command = \[' "${hmEval.config.xdg.configFile."d2b-wlterm/config.toml".source}"
            grep -q '"weezterm"' "${hmEval.config.xdg.configFile."d2b-wlterm/config.toml".source}"
            grep -q 'default_open_behavior = "force-open"' "${hmEval.config.xdg.configFile."d2b-wlterm/config.toml".source}"
            grep -q 'module_name = "custom/d2b-wlterm"' "${hmEval.config.xdg.configFile."d2b-wlterm/config.toml".source}"
            grep -q 'control_center_state_path = "$XDG_RUNTIME_DIR/d2b-wlterm/control-center.json"' "${hmEval.config.xdg.configFile."d2b-wlterm/config.toml".source}"
            printf '%s' '${hmEval.config.xdg.configFile."d2b-wlterm/waybar-module.json".text}' \
              | grep -q '"custom/d2b-wlterm"'
            printf '%s' '${builtins.toJSON hmEval.config.programs.waybar.settings}' \
              | grep -q '"custom/d2b-wlterm"'
            printf '%s' '${builtins.toJSON hmEval.config.programs.waybar.settings}' \
              | grep -q '"modules-right":\["clock","custom/d2b-wlterm"\]'
            printf '%s' '${hmEval.config.xdg.configFile."d2b-wlterm/quickshell-control-center.json".text}' \
              | grep -q '"statePath"'
            touch $out
          '';
        });

      devShells = forAllSystems (system:
        let pkgs = import nixpkgs { inherit system; };
        in {
          default = pkgs.mkShell {
            packages = with pkgs; [ cargo clippy rustc rustfmt nixpkgs-fmt ];
          };
        });

      homeManagerModules.default = import ./nix/home-manager.nix { inherit self; };
    };
}
