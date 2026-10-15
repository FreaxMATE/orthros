{
  description = "orthros: a quiet daily todo journal";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs, ... }:
  let
    systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];
    forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
  in {
    devShells = forAll (pkgs: {
      default = pkgs.mkShell {
        packages = with pkgs; [
          cargo rustc rustfmt clippy rust-analyzer
          nodejs
          pkg-config
        ];

        # Tauri on Linux: WebKitGTK + GTK, plus TLS for the webview and
        # GSettings schemas so GTK doesn't abort on the file chooser etc.
        buildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux (with pkgs; [
          webkitgtk_4_1
          gtk3
          libsoup_3
          glib-networking
          openssl
          librsvg
        ]);

        shellHook = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
          export GIO_MODULE_DIR=${pkgs.glib-networking}/lib/gio/modules/
          export XDG_DATA_DIRS=${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}:${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}:$XDG_DATA_DIRS
        '';
      };
    });
  };
}
