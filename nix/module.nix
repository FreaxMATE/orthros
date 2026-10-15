# NixOS module for the sync server.
#
#   services.orthros-server = {
#     enable = true;
#     tokenFile = "/var/lib/secrets/orthros-token";   # outside the Nix store
#   };
#
# It listens on 127.0.0.1 only: put HTTPS in front, e.g.
#   tailscale serve --bg --https=8444 http://127.0.0.1:8787
# The space lives in /var/lib/orthros (plain markdown files).
self:
{ config, lib, pkgs, ... }:

let
  cfg = config.services.orthros-server;
in
{
  options.services.orthros-server = {
    enable = lib.mkEnableOption "the orthros sync server";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.server;
      defaultText = lib.literalExpression "orthros.packages.\${system}.server";
      description = "The orthros-server package.";
    };

    listenAddress = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1";
      description = "Address to listen on.";
    };

    port = lib.mkOption {
      type = lib.types.port;
      default = 8787;
      description = "Port to listen on.";
    };

    tokenFile = lib.mkOption {
      type = lib.types.str;
      example = "/var/lib/secrets/orthros-token";
      description = ''
        File holding the sync token (at least 16 characters). Every device
        must send it. Keep it out of the Nix store.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.orthros-server = {
      description = "orthros sync server";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ];
      serviceConfig = {
        ExecStart = "${lib.getExe cfg.package} --data /var/lib/orthros --listen ${cfg.listenAddress}:${toString cfg.port} --token-file %d/token";
        LoadCredential = "token:${cfg.tokenFile}";
        DynamicUser = true;
        StateDirectory = "orthros";
        StateDirectoryMode = "0700";
        UMask = "0077";
        Restart = "on-failure";

        # Hardening: it only needs its state directory and the network.
        CapabilityBoundingSet = "";
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        NoNewPrivileges = true;
        PrivateDevices = true;
        PrivateTmp = true;
        ProtectClock = true;
        ProtectControlGroups = true;
        ProtectHome = true;
        ProtectHostname = true;
        ProtectKernelLogs = true;
        ProtectKernelModules = true;
        ProtectKernelTunables = true;
        ProtectSystem = "strict";
        RestrictAddressFamilies = [ "AF_INET" "AF_INET6" ];
        RestrictNamespaces = true;
        RestrictRealtime = true;
        SystemCallArchitectures = "native";
        SystemCallFilter = [ "@system-service" "~@privileged" ];
      };
    };
  };
}
