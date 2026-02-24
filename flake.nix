{
  description = "Sam's small image board";

  inputs = { };

  outputs =
    { ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      eachSystem =
        f:
        (builtins.foldl' (
          acc: system:
          let
            fSystem = f system;
          in
          builtins.foldl' (
            acc': attr:
            acc'
            // {
              ${attr} = (acc'.${attr} or { }) // fSystem.${attr};
            }
          ) acc (builtins.attrNames fSystem)
        ) { } systems);
    in
    eachSystem (
      system:
      let
        inherit ((import ./default.nix { inherit system; })) samey;
      in
      {
        packages.${system} = {
          inherit samey;
          default = samey;
        };

        devShells.${system}.default = import ./shell.nix { inherit system; };
      }
    );
}
