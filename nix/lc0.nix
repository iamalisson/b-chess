# lc0 built with oneDNN: the nixpkgs build only has the slower Eigen backend,
# which matters for levels 2200+.
{ pkgs }:
let
  dnnl = pkgs.symlinkJoin {
    name = "dnnl";
    paths = [ pkgs.oneDNN.dev pkgs.oneDNN.out ];
  };
in
pkgs.lc0.overrideAttrs (old: {
  buildInputs = old.buildInputs ++ [ pkgs.oneDNN ];
  mesonFlags = old.mesonFlags ++ [ "-Ddnnl=true" "-Ddnnl_dir=${dnnl}" ];
})
