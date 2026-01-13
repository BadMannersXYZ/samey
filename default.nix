let
  inherit (import ./npins)
    nixpkgs
    rust-overlay
    ;
  pkgs = import nixpkgs {
    overlays = [ (import rust-overlay) ];
  };
  inherit (pkgs) lib stdenv;

  # Options for your package
  pname = "samey";
  version = "0.1.0";
  docker-image = "badmanners/samey";
  cargo-deps-hash = "sha256-oiz2a6Vip199saU/s/sBn/3Cl0eJaSltN3n1uPETHGk=";
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

  rust-bin = pkgs.rust-bin.stable.latest.default.override {
    targets = [
      "x86_64-unknown-linux-gnu"
      "aarch64-unknown-linux-gnu"
    ];
  };

  mkRustPkg =
    target:
    stdenv.mkDerivation {
      inherit
        pname
        version
        src
        ;

      cargoDeps = pkgs.rustPlatform.fetchCargoVendor {
        inherit src;
        hash = cargo-deps-hash;
      };

      nativeBuildInputs = [
        pkgs.rustPlatform.cargoSetupHook
        pkgs.zig
        rust-bin
      ];

      buildPhase = ''
        export HOME=$(mktemp -d)
        ${pkgs.cargo-zigbuild}/bin/cargo-zigbuild zigbuild --release --target ${target}
      '';

      installPhase = ''
        mkdir -p $out/bin
        cp ./target/${target}/release/${pname} $out/bin/
      '';
    };

  amd64 = {
    system = "x86_64-linux";
    pkgs = import nixpkgs {
      localSystem = "x86_64-linux";
    };
    tag = "latest-amd64";
    target = "x86_64-unknown-linux-gnu";
  };

  arm64 = {
    system = "aarch64-linux";
    pkgs = import nixpkgs {
      localSystem = "aarch64-linux";
    };
    tag = "latest-arm64";
    target = "aarch64-unknown-linux-gnu";
  };

  mkDocker =
    targetAttrs:
    let
      pkgs-cross =
        if targetAttrs.system == builtins.currentSystem then
          pkgs
        else
          (import nixpkgs {
            crossSystem = targetAttrs.system;
          });
      rust-package = mkRustPkg targetAttrs.target;
    in
    pkgs-cross.dockerTools.buildLayeredImage {
      name = docker-image;
      inherit (targetAttrs) tag;
      contents = [
        targetAttrs.pkgs.ffmpeg-headless
      ];
      config.Entrypoint = [
        "${rust-package}/bin/${pname}"
      ];
    };
in
{
  docker-amd64 = mkDocker amd64;
  docker-arm64 = mkDocker arm64;
}
