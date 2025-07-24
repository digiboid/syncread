{
  description = "SyncRead - Synchronized Media Viewer";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };

        buildInputs = with pkgs; [
          # Rust toolchain
          rustToolchain

          # Media and system libraries
          mpv
          pkg-config
          openssl

          # Networking libraries (for potential WebRTC/P2P needs)
          libwebrtc
          
          # Development tools
          git
          just  # task runner alternative to make
          
          # Optional: for debugging network issues
          wireshark
          netcat-gnu
        ];

        nativeBuildInputs = with pkgs; [
          pkg-config
        ];

      in
      {
        devShells.default = pkgs.mkShell {
          inherit buildInputs nativeBuildInputs;
          
          # Environment variables
          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
          PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig";
          
          shellHook = ''
            echo "🦀 Welcome to SyncRead development environment!"
            echo "Available tools:"
            echo "  - Rust $(rustc --version)"
            echo "  - MPV $(mpv --version | head -1)"
            echo "  - Cargo $(cargo --version)"
            echo ""
            echo "Quick commands:"
            echo "  cargo run          - Run the application"
            echo "  cargo test         - Run tests"
            echo "  cargo check        - Check code without building"
            echo "  just --list        - Show available tasks (if you add justfile)"
            echo ""
            echo "MPV socket will typically be at: /tmp/mpvsocket"
          '';
        };

        # Optional: define packages for building releases
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "syncread";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          
          inherit buildInputs nativeBuildInputs;
        };
      });
}
