{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  nativeBuildInputs = with pkgs; [
    rustc
    cargo
    rustfmt
    clippy
    rust-analyzer
    gcc
    gnumake
    pkg-config
    cmake
    openssl
  ];

  shellHook = ''
    # Unset LD_LIBRARY_PATH if present to prevent GLIBC mismatches between host OS and Nix packages
    unset LD_LIBRARY_PATH

    echo ""
    echo "  Rust + Axum development shell"
    echo "  ─────────────────────────────"
    echo "  gunakan \`cargo build\` / \`cargo run\` seperti biasa"
    echo ""
  '';
}
