# The desktop app (Linux). macOS: build with `npm run tauri build` instead.
{
  lib,
  rustPlatform,
  importNpmLock,
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
  # Like cargoLock: every package's checksum comes from the lock file, so
  # there is no hash here to update when dependencies or the version change.
  npmDeps = importNpmLock {
    package = lib.importJSON ../package.json;
    packageLock = lib.importJSON ../package-lock.json;
  };

  nativeBuildInputs = [
    cargo-tauri.hook
    nodejs
    importNpmLock.npmConfigHook
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
