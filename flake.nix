{
  description = "Wayland terminal launcher companion for d2b";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    d2b-client-toolkit = {
      url = "github:vicondoa/d2b-toolkit/3d6b75d47c8df66c1722ea324d64334a127d43ec";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, d2b-client-toolkit }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      version = "2.0.0";
      runtimeBins = pkgs: with pkgs; [ quickshell ];
      runtimeFonts = pkgs: with pkgs; [ material-symbols ];
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
          toolkitSource = d2b-client-toolkit.packages.${system}.default;
        in
        {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "d2b-wlterm";
            inherit version;
            src = pkgs.lib.cleanSource ./.;
            nativeBuildInputs = with pkgs; [ makeWrapper ];
            postPatch = ''
              substituteInPlace Cargo.toml \
                --replace-fail "../d2b-client-toolkit/crates/d2b-client-toolkit" \
                  "${toolkitSource}/share/d2b-client-toolkit/distribution/crates/d2b-client-toolkit"
            '';
            postInstall = ''
              wrapProgram "$out/bin/d2b-wlterm" \
                --prefix PATH : ${pkgs.lib.makeBinPath (runtimeBins pkgs)} \
                --prefix XDG_DATA_DIRS : ${pkgs.lib.makeSearchPath "share" (runtimeFonts pkgs)}
            '';
            cargoLock = {
              lockFile = ./Cargo.lock;
              outputHashes = {
                "d2b-client-2.0.0" = "sha256-mDNv+gkV0GKOFDWJEunuR76mPIwQsSg9AJcxsI5qhMQ=";
                "d2b-contracts-2.0.0" = "sha256-mDNv+gkV0GKOFDWJEunuR76mPIwQsSg9AJcxsI5qhMQ=";
                "d2b-session-2.0.0" = "sha256-mDNv+gkV0GKOFDWJEunuR76mPIwQsSg9AJcxsI5qhMQ=";
                "d2b-session-unix-2.0.0" = "sha256-mDNv+gkV0GKOFDWJEunuR76mPIwQsSg9AJcxsI5qhMQ=";
              };
            };
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
          release-metadata = pkgs.runCommand "d2b-wlterm-release-metadata-${version}" { } ''
            grep -Fq 'version = "2.0.0"' ${./Cargo.toml}
            grep -Fq 'revision = "3d6b75d47c8df66c1722ea324d64334a127d43ec"' ${./Cargo.toml}
            grep -Fq 'd2b-source-revision = "9dc902243cdd7aba7ef269988b96f0aae6e037da"' ${./Cargo.toml}
            grep -Fq 'source-inventory-sha256 = "35c33c2e23e1b9f03b5abc3bbca2d3320e38c42dfc7aceb7e3476d28210cde8c"' ${./Cargo.toml}
            grep -Fq '3d6b75d47c8df66c1722ea324d64334a127d43ec' ${./flake.lock}
            touch $out
          '';
          home-manager-module = pkgs.runCommand "d2b-wlterm-home-manager-module-${version}" { } ''
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
            printf '%s' '${hmEval.config.xdg.configFile."d2b-wlterm/quickshell-control-center.json".text}' \
              | grep -qv '"actions"'
            touch $out
          '';
        });

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/d2b-wlterm";
        };
      });

      devShells = forAllSystems (system:
        let pkgs = import nixpkgs { inherit system; };
        in {
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              clippy
              rustc
              rustfmt
              nixpkgs-fmt
              quickshell
              material-symbols
            ];
          };
        });

      homeManagerModules.default = import ./nix/home-manager.nix { inherit self; };
    };
}
