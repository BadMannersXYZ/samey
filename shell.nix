{
  system ? builtins.currentSystem,
}:
let
  inherit (import ./npins)
    nixpkgs
    rust-overlay
    ;
  pkgs = import nixpkgs {
    inherit system;
    overlays = [ (import rust-overlay) ];
  };
in
pkgs.mkShell {
  packages = [
    pkgs.bacon
    pkgs.ffmpeg_8-headless
    pkgs.just
    pkgs.rust-bin.stable.latest.default
    pkgs.sea-orm-cli
  ];
}
