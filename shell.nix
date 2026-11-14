{
  system ? builtins.currentSystem,
  inputs ? import ./.tack,
  pkgs ? import inputs.nixpkgs { inherit system; },
}:
pkgs.mkShell {
  packages = [
    pkgs.bacon
    pkgs.cargo
    pkgs.clippy
    pkgs.ffmpeg_8-headless
    pkgs.just
    pkgs.rustc
    pkgs.rustfmt
    pkgs.sea-orm-cli
  ];
}
