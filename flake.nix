{
  description = "Blindfold chess trainer for the terminal, playing Lc0";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      forAll = f: nixpkgs.lib.genAttrs [ "x86_64-linux" "aarch64-linux" ]
        (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAll (pkgs: rec {
        lc0 = import ./nix/lc0.nix { inherit pkgs; };
        b-chess = pkgs.rustPlatform.buildRustPackage {
          pname = "b-chess";
          version = "0.1.0";
          src = self;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [ pkgs.makeWrapper ];
          # Ships with its own lc0, and curl for the first-run network download.
          postInstall = ''
            wrapProgram $out/bin/b-chess \
              --set-default LC0 ${lc0}/bin/lc0 \
              --prefix PATH : ${pkgs.curl}/bin
          '';
          meta.mainProgram = "b-chess";
        };
        default = b-chess;
      });

      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = [ self.packages.${pkgs.stdenv.hostPlatform.system}.lc0 pkgs.cargo pkgs.rustc ];
        };
      });
    };
}
