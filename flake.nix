{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };
  outputs =
    {
      self,
      nixpkgs,
      utils,
      rust-overlay,
    }:
    utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };

        # Get the Rust toolchain
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [
            "rust-src"
            "rust-analyzer"
          ];
        };
      in
      rec {
        packages.syncread = pkgs.rustPlatform.buildRustPackage {
          pname = "syncread";
          version = "0.1.0";
          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          nativeBuildInputs = with pkgs; [
            pkg-config
          ];

          buildInputs = with pkgs; [
            openssl
          ];
          
          # MPV is a runtime dependency - we spawn it as external process
          # Keep it in dev shell for development/testing
          # Users need to install MPV separately
        };

        packages.default = packages.syncread;

        apps.syncread = utils.lib.mkApp {
          drv = packages.syncread;
        };
        apps.default = apps.syncread;

        devShells.default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            rustToolchain
            pkg-config
            mpv
            openssl
          ];

          shellHook = ''
            export RUST_SRC_PATH="${rustToolchain}/lib/rustlib/src/rust/library"
            echo "🦀 Welcome to SyncRead development environment!"
            echo "Available tools:"
            echo "  - Rust $(rustc --version)"
            echo "  - MPV $(mpv --version | head -1)"
            echo "  - Cargo $(cargo --version)"
          '';
        };
      }
    );
}