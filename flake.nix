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
        inherit (pkgs) lib;

        # One declarative toolchain. `.default` already bundles cargo, rustc,
        # rustfmt, clippy, and rust-docs; we add the two extras a dev workflow wants.
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };

        # Build third-party Rust tools with the toolchain pinned above rather than
        # nixpkgs' own rustc, so the shell only ever contains one Rust.
        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };

        # The Sprinkles visual particle editor — design .ron effects, save them into
        # assets/, spawn them via Particles3d. It ships as its own crate whose binary
        # is named `sprinkles`. Beware: the crates.io package literally named
        # `sprinkles` is an unrelated terminal text-colorizer; this is the right one.
        sprinkles-editor = rustPlatform.buildRustPackage rec {
          pname = "bevy_sprinkles_editor";
          version = "0.3.0";

          src = pkgs.fetchCrate {
            inherit pname version;
            hash = "sha256-jO88ERFrobQ5LBXNNtsKO0LkAsHCVWg9kg3EIN13m9g=";
          };

          cargoHash = "sha256-XGVNYV+Zp2CyjVOMNH4dZnAXMmpNc7MWVYOXOxmWTMw=";

          nativeBuildInputs = [ pkgs.pkg-config ];

          buildInputs =
            lib.optionals pkgs.stdenv.hostPlatform.isDarwin [ pkgs.apple-sdk ]
            ++ lib.optionals pkgs.stdenv.hostPlatform.isLinux [
              pkgs.alsa-lib
              pkgs.libxkbcommon
              pkgs.udev
              pkgs.vulkan-loader
              pkgs.wayland
            ];

          # A GUI app — it ships no tests worth running in the build sandbox.
          doCheck = false;

          meta = {
            description = "GPU particle system editor for Bevy (Sprinkles)";
            homepage = "https://github.com/doceazedo/sprinkles";
            license = with lib.licenses; [ mit asl20 ];
            mainProgram = "sprinkles";
          };
        };
      in
      {
        # `nix build .#sprinkles-editor` also works if you just want the binary.
        packages.sprinkles-editor = sprinkles-editor;

        devShells.default = pkgs.mkShell {
          # Everything here lands on PATH inside `nix develop`.
          packages = [
            rustToolchain
            pkgs.pkg-config
            sprinkles-editor
          ];

          # A banner so you can see the shell actually loaded, and which rustc you got.
          shellHook = ''
            echo "orgonic dev shell — $(rustc --version)"
          '';
        };
      });
}
