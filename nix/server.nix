# The sync server.
{
  lib,
  rustPlatform,
  src,
  version,
}:

rustPlatform.buildRustPackage {
  pname = "orthros-server";
  inherit version src;

  cargoLock.lockFile = ../Cargo.lock;
  cargoBuildFlags = [ "-p" "orthros-server" ];
  cargoTestFlags = [ "-p" "orthros-server" "-p" "orthros-core" ];

  meta = {
    description = "Sync server for orthros";
    homepage = "https://github.com/FreaxMATE/orthros";
    license = lib.licenses.agpl3Only;
    mainProgram = "orthros-server";
    platforms = lib.platforms.unix;
  };
}
