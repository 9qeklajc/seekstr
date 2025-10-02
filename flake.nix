{
  description = "Scribe - Media file transcription and description tool";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };

        nativeBuildInputs = with pkgs; [
          rustToolchain
          pkg-config
          cmake
          clang
          llvmPackages.libclang
          llvmPackages.libcxxClang
          protobuf
        ];

        buildInputs = with pkgs; [
          openssl
          # For whisper.cpp
          blas
          lapack
          # Optional: CUDA support (uncomment if needed)
          # cudatoolkit
        ] ++ lib.optionals stdenv.isDarwin [
          darwin.apple_sdk.frameworks.CoreServices
          darwin.apple_sdk.frameworks.CoreFoundation
          darwin.apple_sdk.frameworks.Security
        ];

        # Environment variables for building whisper-rs
        LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
        BINDGEN_EXTRA_CLANG_ARGS = "-I${pkgs.llvmPackages.libclang.lib}/lib/clang/${pkgs.llvmPackages.libclang.version}/include";

      in
      {
        devShells.default = pkgs.mkShell {
          inherit buildInputs nativeBuildInputs;

          shellHook = ''
            echo "Scribe development environment"
            echo "================================"
            echo "Build commands:"
            echo "  cargo build                     # Build without Whisper support"
            echo "  cargo build --features whisper  # Build with Whisper support"
            echo "  cargo run -- --help            # Show help"
            echo ""
            echo "Environment variables set:"
            echo "  LIBCLANG_PATH=${LIBCLANG_PATH}"
            echo ""
            export LIBCLANG_PATH="${LIBCLANG_PATH}"
            export BINDGEN_EXTRA_CLANG_ARGS="${BINDGEN_EXTRA_CLANG_ARGS}"
          '';
        };

        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "scribe";
          version = "0.1.0";
          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          inherit nativeBuildInputs buildInputs;

          # Build with all features
          buildFeatures = [ "whisper" ];

          # Set environment variables for the build
          LIBCLANG_PATH = "${LIBCLANG_PATH}";
          BINDGEN_EXTRA_CLANG_ARGS = "${BINDGEN_EXTRA_CLANG_ARGS}";

          meta = with pkgs.lib; {
            description = "Dead-simple media file transcription and description tool";
            homepage = "https://github.com/yourusername/scribe";
            license = licenses.mit;
            maintainers = [];
          };
        };

        packages.scribe-no-whisper = pkgs.rustPlatform.buildRustPackage {
          pname = "scribe";
          version = "0.1.0";
          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          nativeBuildInputs = with pkgs; [
            rustToolchain
            pkg-config
          ];

          buildInputs = with pkgs; [
            openssl
          ] ++ lib.optionals stdenv.isDarwin [
            darwin.apple_sdk.frameworks.CoreServices
            darwin.apple_sdk.frameworks.CoreFoundation
            darwin.apple_sdk.frameworks.Security
          ];

          # Build without whisper feature
          buildFeatures = [];

          meta = with pkgs.lib; {
            description = "Dead-simple media file transcription and description tool (without Whisper)";
            homepage = "https://github.com/yourusername/scribe";
            license = licenses.mit;
            maintainers = [];
          };
        };

        apps.default = flake-utils.lib.mkApp {
          drv = self.packages.${system}.default;
        };
      });
}