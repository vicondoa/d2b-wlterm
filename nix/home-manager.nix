{ self ? null }:
{ config, lib, pkgs, ... }:

let
  cfg = config.programs.d2b-wlterm;
  tomlFormat = pkgs.formats.toml { };
  packageForSystem =
    if self != null && self ? packages && self.packages ? ${pkgs.stdenv.hostPlatform.system}
    then self.packages.${pkgs.stdenv.hostPlatform.system}.default
    else null;
  baseSettings = {
    public_socket_path = cfg.publicSocketPath;
    wezterm_command = cfg.weztermCommand;
    refresh_interval_seconds = cfg.refreshIntervalSeconds;
    ui = {
      default_open_behavior = cfg.defaultOpenBehavior;
      stop_confirmation = cfg.stopConfirmation;
      async_error_display = cfg.asyncErrorDisplay;
    };
    waybar = {
      enable = cfg.waybar.enable;
      module_name = cfg.waybar.moduleName;
    };
  };
  renderedSettings = lib.recursiveUpdate baseSettings cfg.settings;
  waybarModule = lib.recursiveUpdate {
    return-type = "json";
    exec = if cfg.package != null then "${lib.getExe cfg.package} waybar" else "";
    interval = cfg.refreshIntervalSeconds;
    tooltip = true;
  } cfg.waybar.module;
in
{
  options.programs.d2b-wlterm = {
    enable = lib.mkEnableOption "d2b Wayland terminal launcher";

    package = lib.mkOption {
      type = lib.types.nullOr lib.types.package;
      default = packageForSystem;
      defaultText = lib.literalExpression "inputs.d2b-wlterm.packages.${pkgs.stdenv.hostPlatform.system}.default";
      description = "Package providing the d2b-wlterm CLI.";
    };

    publicSocketPath = lib.mkOption {
      type = lib.types.str;
      default = "$XDG_RUNTIME_DIR/d2b/public.sock";
      description = "Path to the d2b public daemon socket used by the launcher.";
    };

    weztermCommand = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "weezterm" "start" "--" ];
      description = "Command prefix used when opening a terminal window.";
    };

    refreshIntervalSeconds = lib.mkOption {
      type = lib.types.ints.positive;
      default = 5;
      description = "Default refresh interval for status polling and Waybar output.";
    };

    defaultOpenBehavior = lib.mkOption {
      type = lib.types.enum [ "focus-existing" "force-open" "prompt" ];
      default = "focus-existing";
      description = "Default UI behavior when a terminal is already attached.";
    };

    stopConfirmation = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Whether Stop actions require UI confirmation before dispatch.";
    };

    asyncErrorDisplay = lib.mkOption {
      type = lib.types.enum [ "inline" "notification" "waybar" "silent" ];
      default = "notification";
      description = "How delayed d2b/compositor errors should be surfaced.";
    };

    settings = lib.mkOption {
      type = tomlFormat.type;
      default = { };
      description = "Additional raw TOML settings merged into the generated config.";
    };

    waybar = {
      enable = lib.mkEnableOption "d2b-wlterm Waybar module snippet";

      moduleName = lib.mkOption {
        type = lib.types.str;
        default = "custom/d2b-wlterm";
        description = "Suggested Waybar module name.";
      };

      module = lib.mkOption {
        type = lib.types.attrsOf lib.types.anything;
        default = { };
        description = "Additional attributes merged into the rendered Waybar module snippet.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.package != null;
        message = "programs.d2b-wlterm.package must be set when the module is not imported from the d2b-wlterm flake";
      }
    ];

    home.packages = [ cfg.package ];

    xdg.configFile."d2b-wlterm/config.toml".source =
      tomlFormat.generate "d2b-wlterm-config.toml" renderedSettings;

    xdg.configFile."d2b-wlterm/waybar-module.json" = lib.mkIf cfg.waybar.enable {
      text = builtins.toJSON { ${cfg.waybar.moduleName} = waybarModule; } + "\n";
    };
  };
}
