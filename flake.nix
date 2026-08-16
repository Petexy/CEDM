{
  description = "Console Experience Desktop Manager";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs = { nixpkgs, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
          cedm = pkgs.callPackage ./packaging/nix/package.nix { src = ./.; };
        in
        {
          cedm = cedm;
          default = cedm;
        });

      nixosModules = {
        cedm = import ./packaging/nix/module.nix;
        default = import ./packaging/nix/module.nix;
      };
    };
}
