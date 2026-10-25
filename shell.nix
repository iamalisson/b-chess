# nix-shell: lc0 for running from source. Rust comes from rustup or your system.
{ pkgs ? import <nixpkgs> { } }:
pkgs.mkShell {
  packages = [ (import ./nix/lc0.nix { inherit pkgs; }) ];
}
