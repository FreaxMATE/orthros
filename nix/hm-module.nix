# Home Manager module for the app.
#
#   programs.orthros = {
#     enable = true;
#     sync.server = "https://my-server.example.ts.net:8444";   # optional
#   };
#
# Installs Orthros, writes sync.json (the token itself stays out of the Nix
# store, in sync.tokenFile) and, on GNOME, binds Ctrl+Alt+Space to quick
# capture.
self:
{ config, lib, pkgs, ... }:

let
  cfg = config.programs.orthros;
  inherit (pkgs.stdenv.hostPlatform) isDarwin isLinux system;
  syncJson = builtins.toJSON {
    server = cfg.sync.server;
    token_file = cfg.sync.tokenFile;
  };
  keybinding = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/orthros-capture/";
in
{
  options.programs.orthros = {
    enable = lib.mkEnableOption "Orthros, a quiet daily todo journal";

    package = lib.mkOption {
      type = lib.types.nullOr lib.types.package;
      default = self.packages.${system}.orthros or null;
      defaultText = lib.literalExpression "orthros.packages.\${system}.orthros";
      description = "The Orthros package, or null to only write the configuration (macOS: install the .dmg).";
    };

    sync = {
      server = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        example = "https://my-server.example.ts.net:8444";
        description = "URL of your orthros-server. Null: no sync, Orthros stays local.";
      };

      tokenFile = lib.mkOption {
        type = lib.types.str;
        default = if isDarwin then "~/Library/Application Support/orthros/token" else "~/.config/orthros/token";
        description = "File holding the server's token. Create it yourself (chmod 600); it is never put in the Nix store.";
      };
    };

    captureShortcut = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = "<Control><Alt>space";
      description = ''
        GNOME shortcut that opens quick capture. Null leaves GNOME's
        shortcuts alone. Note that this sets GNOME's custom-keybindings list,
        so other custom shortcuts must be declared in Home Manager too.
      '';
    };
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    { home.packages = lib.optional (cfg.package != null) cfg.package; }

    (lib.mkIf (cfg.sync.server != null && !isDarwin) {
      xdg.configFile."orthros/sync.json".text = syncJson;
    })

    (lib.mkIf (cfg.sync.server != null && isDarwin) {
      home.file."Library/Application Support/orthros/sync.json".text = syncJson;
    })

    (lib.mkIf (cfg.captureShortcut != null && isLinux) {
      dconf.settings = {
        "org/gnome/settings-daemon/plugins/media-keys".custom-keybindings = [ keybinding ];
        "org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/orthros-capture" = {
          name = "Orthros: capture a thought";
          command = "orthros --capture";
          binding = cfg.captureShortcut;
        };
      };
    })
  ]);
}
