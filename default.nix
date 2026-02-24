{
  system ? builtins.currentSystem,
}:
let
  inherit (import ./npins)
    nixpkgs
    rust-overlay
    ;
  currentPkgs = import nixpkgs {
    inherit system;
    overlays = [ (import rust-overlay) ];
  };
  inherit (currentPkgs) lib stdenv;

  crate-info = fromTOML (builtins.readFile ./Cargo.toml);
  pname = crate-info.package.name;
  version = crate-info.package.version;
  description = crate-info.package.description;

  docker-image = "badmanners/${pname}";
  cargo-deps-hash = "sha256-w8D50YDFC9K7dfXKEIrsuAF0cvcvotw2O4JWoPBnHJ0=";
  cargo-src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./Cargo.toml
      ./Cargo.lock
    ];
  };
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./Cargo.toml
      ./Cargo.lock
      ./src
      ./migration/Cargo.toml
      ./migration/src
      ./static
      ./templates
    ];
  };

  archs = {
    amd64 = {
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        localSystem = "x86_64-linux";
      };
      tag = "latest-amd64";
      targetTriple = "x86_64-unknown-linux-musl";
    };
    arm64 = {
      system = "aarch64-linux";
      pkgs = import nixpkgs {
        localSystem = "aarch64-linux";
      };
      tag = "latest-arm64";
      targetTriple = "aarch64-unknown-linux-musl";
    };
  };

  rust-bin = currentPkgs.rust-bin.stable.latest.default.override {
    targets = [
      "x86_64-unknown-linux-musl"
      "aarch64-unknown-linux-musl"
    ];
  };

  mkRustPkg =
    targetTriple:
    (stdenv.mkDerivation {
      inherit
        pname
        version
        src
        ;

      cargoDeps = currentPkgs.rustPlatform.fetchCargoVendor {
        src = cargo-src;
        hash = cargo-deps-hash;
      };

      nativeBuildInputs = [
        currentPkgs.rustPlatform.cargoSetupHook
        currentPkgs.zig
        rust-bin
      ];

      buildPhase = ''
        export HOME=$(mktemp -d)
        ${currentPkgs.cargo-zigbuild}/bin/cargo-zigbuild zigbuild --release --target ${targetTriple}
      '';

      installPhase = ''
        mkdir -p $out/bin
        cp ./target/${targetTriple}/release/${pname} $out/bin/
      '';

      meta = {
        inherit description;
        mainProgram = pname;
      };
    });

  mkDocker =
    {
      system,
      pkgs,
      tag,
      targetTriple,
    }:
    let
      pkgs-cross =
        if system == builtins.currentSystem then
          currentPkgs
        else
          (import nixpkgs {
            crossSystem = system;
          });
      rust-package = mkRustPkg targetTriple;
      ffmpeg = pkgs.ffmpeg_8-headless;
    in
    pkgs-cross.dockerTools.buildLayeredImage {
      name = docker-image;
      inherit tag;
      contents = [ ffmpeg ];
      config.Entrypoint = [ (lib.getExe rust-package) ];
    };

  currentTargetTriple =
    {
      system ? system,
    }:
    (lib.lists.findFirst (arch: arch.system == system) {
      targetTriple = throw "Unknown current system ${system}";
    } (lib.attrValues archs)).targetTriple;

  samey = mkRustPkg (currentTargetTriple {
    inherit system;
  });
in
{
  inherit samey;
  docker-amd64 = mkDocker archs.amd64;
  docker-arm64 = mkDocker archs.arm64;
}
