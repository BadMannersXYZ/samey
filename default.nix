{
  system ? builtins.currentSystem,
  inputs ? import ./.tack,
  pkgs ? import inputs.nixpkgs { inherit system; },
}:
let
  crate-info = fromTOML (builtins.readFile ./Cargo.toml);
  pname = crate-info.package.name;
  version = crate-info.package.version;
  description = crate-info.package.description;

  src = pkgs.lib.fileset.toSource {
    root = ./.;
    fileset = pkgs.lib.fileset.unions [
      ./Cargo.toml
      ./Cargo.lock
      ./src
      ./migration/Cargo.toml
      ./migration/src
      ./static
      ./templates
    ];
  };

  samey = pkgs.rustPlatform.buildRustPackage {
    inherit pname version src;

    cargoLock.lockFile = ./Cargo.lock;

    nativeBuildInputs = [
      pkgs.makeWrapper
      pkgs.pkg-config
    ];

    buildInputs = [ pkgs.sqlite ];

    postInstall = ''
      wrapProgram $out/bin/samey \
        --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.ffmpeg_8-headless ]}
    '';

    doCheck = false;

    meta = {
      inherit description;
      mainProgram = "samey";
    };
  };
in
{
  inherit samey;
}
