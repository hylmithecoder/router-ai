# Pinned Python 3.12 environment for the local ALPR engine.
#
# Why 3.12 and not the system interpreter: onnxruntime ships no wheel for the
# Python 3.14 in the user's nix-profile, so `import onnxruntime` fails and every
# ONNX model silently falls back to nothing.
#
# Usage:
#   nix-shell python-alpr-local/shell.nix --run python-alpr-local/setup.sh
{ pkgs ? import <nixpkgs> { } }:

let
  # Binary wheels (onnxruntime, opencv, numpy) are not patched for NixOS, so the
  # loader needs these libraries on LD_LIBRARY_PATH at runtime.
  runtimeLibs = with pkgs; [
    stdenv.cc.cc.lib
    zlib
    glib
    libGL
    glibc
  ];
in
pkgs.mkShell {
  buildInputs = [ pkgs.python312 ] ++ runtimeLibs;

  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
}
