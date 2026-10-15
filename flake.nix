{
  description = "orthros: a quiet daily todo journal";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs, ... }:
  let
    lib = nixpkgs.lib;
    systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];
    forAll = f: lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    version = (lib.importTOML ./Cargo.toml).workspace.package.version;

    # Only what the builds need: no node_modules, target/, dist/.
    src = lib.fileset.toSource {
      root = ./.;
      fileset = lib.fileset.unions [
        ./Cargo.toml
        ./Cargo.lock
        ./core
        ./server
        ./src-tauri/Cargo.toml
        ./src-tauri/build.rs
        ./src-tauri/tauri.conf.json
        ./src-tauri/capabilities
        ./src-tauri/icons
        ./src-tauri/src
        ./src
        ./index.html
        ./capture.html
        ./package.json
        ./package-lock.json
        ./tsconfig.json
        ./vite.config.ts
      ];
    };
  in {
    packages = forAll (pkgs:
      {
        server = pkgs.callPackage ./nix/server.nix { inherit src version; };
      }
      // lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux rec {
        orthros = pkgs.callPackage ./nix/package.nix { inherit src version; };
        default = orthros;
      });

    nixosModules.server = import ./nix/module.nix self;
    homeManagerModules.default = import ./nix/hm-module.nix self;

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
          # The tray icon loads libayatana-appindicator at runtime.
          export LD_LIBRARY_PATH=${pkgs.lib.makeLibraryPath [ pkgs.libayatana-appindicator ]}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
        '';
      };
    });
  };
}
