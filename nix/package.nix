# The desktop app (Linux). macOS: build with `npm run tauri build` instead.
{
  lib,
  rustPlatform,
  fetchNpmDeps,
  npmHooks,
  nodejs,
  cargo-tauri,
  pkg-config,
  wrapGAppsHook3,
  webkitgtk_4_1,
  glib-networking,
  openssl,
  libayatana-appindicator,
  src,
  version,
}:

rustPlatform.buildRustPackage (finalAttrs: {
  pname = "orthros";
  inherit version src;

  cargoLock.lockFile = ../Cargo.lock;
  npmDeps = fetchNpmDeps {
    name = "orthros-${version}-npm-deps";
    inherit src;
    hash = "sha256-eG0uifBX6qUcELtLDo9BHHoqB6Yn/P+Kld5Fhvibk2s=";
  };

  nativeBuildInputs = [
    cargo-tauri.hook
    nodejs
    npmHooks.npmConfigHook
    pkg-config
    wrapGAppsHook3
  ];

  buildInputs = [
    webkitgtk_4_1
    glib-networking
    openssl
    # Found by the bundler at build time, loaded by the tray at runtime.
    libayatana-appindicator
  ];

  buildAndTestSubdir = "src-tauri";

  # The tests run in development (`cargo test`); here we only package.
  doCheck = false;

  # The tray icon loads libayatana-appindicator at runtime.
  preFixup = ''
    gappsWrapperArgs+=(--prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath [ libayatana-appindicator ]})
  '';

  meta = {
    description = "A quiet daily todo journal";
    homepage = "https://github.com/FreaxMATE/orthros";
    license = lib.licenses.agpl3Only;
    mainProgram = "orthros";
    platforms = lib.platforms.linux;
  };
})
