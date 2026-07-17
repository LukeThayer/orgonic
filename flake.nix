{
  description = "Orgonic — a Bevy game, with a reproducible Nix dev shell";

  inputs = {
    # nixpkgs-unstable: freshest system libraries and macOS SDK for a non-NixOS Mac.
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    # oxalica's rust-overlay gives us an exact, pinnable Rust toolchain.
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";

    # Removes per-architecture boilerplate (aarch64 + x86_64 Macs from one definition).
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        # One declarative toolchain. `.default` already bundles cargo, rustc,
        # rustfmt, clippy, and rust-docs; we add the two extras a dev workflow wants.
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };
      in
      {
        devShells.default = pkgs.mkShell {
          # Everything here lands on PATH inside `nix develop`.
          packages = [
            rustToolchain
            pkgs.pkg-config
          ];

          # A banner so you can see the shell actually loaded, and which rustc you got.
          shellHook = ''
            echo "orgonic dev shell — $(rustc --version)"
          '';
        };
      });
}
