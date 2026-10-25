{
  description = "Blindfold chess trainer for the terminal, playing Lc0";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      inherit (nixpkgs) lib;
      cargo = (lib.importTOML ./Cargo.toml).package;
      # Only x86_64-linux is built and tested so far.
      forAll = f: lib.genAttrs [ "x86_64-linux" ] (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAll (pkgs: rec {
        lc0 = import ./nix/lc0.nix { inherit pkgs; };
        b-chess = pkgs.rustPlatform.buildRustPackage {
          pname = cargo.name;
          inherit (cargo) version;
          # Just what the build needs: docs, screenshots and local data/ never reach the store.
          src = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [ ./Cargo.toml ./Cargo.lock ./src ];
          };
          cargoLock.lockFile = ./Cargo.lock;
          # Keep files in ~/.local/share/b-chess, never in the build directory.
          env.B_CHESS_PACKAGED = "1";
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
          inputsFrom = [ self.packages.${pkgs.stdenv.hostPlatform.system}.b-chess ];
          packages = [ self.packages.${pkgs.stdenv.hostPlatform.system}.lc0 ];
        };
      });
    };
}
