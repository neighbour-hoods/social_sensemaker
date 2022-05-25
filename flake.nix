{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    holonix = {
      url = "github:holochain/holonix";
      flake = false;
    };
    rust-overlay.url = "github:oxalica/rust-overlay";
    cargo2nix.url = "github:cargo2nix/cargo2nix";
    flake-compat = {
      url = "github:edolstra/flake-compat";
      flake = false;
    };
    naersk.url = "github:nix-community/naersk";
  };

  outputs = { nixpkgs, flake-utils, holonix, rust-overlay, cargo2nix, naersk, ... }:
    flake-utils.lib.eachSystem ["x86_64-linux" "aarch64-linux" "x86_64-darwin"] (system:
      let
        holonixMain = import holonix {
          holochainVersionId = "v0_0_127";
        };

        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlay ];
        };

        rustVersion = "1.60.0";

      in

      {
        devShell = pkgs.mkShell {
          inputsFrom = [
            holonixMain.main
          ];

          buildInputs = [
            holonixMain.pkgs.binaryen
          ] ++ (with pkgs; [
            miniserve
            nodePackages.rollup
            wasm-pack
            cargo2nix.defaultPackage.${system}
          ]);

          shellHook = ''
            export CARGO_HOME=~/.cargo
            export CARGO_TARGET_DIR=target
          '';
        };

        packages.holonix = holonixMain;

        packages.social_sensemaker-cargo2nix =
          let
            # create nixpkgs that contains rustBuilder from cargo2nix overlay
            crossPkgs = import nixpkgs {
              inherit system;

              crossSystem = {
                config = "wasm32-unknown-wasi";
                system = "wasm32-wasi";
                useLLVM = true;
              };

              overlays = [
                (import "${cargo2nix}/overlay")
                rust-overlay.overlay
              ];
            };

            # create the workspace & dependencies package set
            rustPkgs = crossPkgs.rustBuilder.makePackageSet' {
              rustChannel = rustVersion;
              packageFun = import ./crates/social_sensemaker/Cargo.nix;
              target = "wasm32-unknown-unknown";
            };

          in

          rustPkgs.workspace.social_sensemaker {};

        packages.social_sensemaker-naersk =
          let
            wasmTarget = "wasm32-unknown-unknown";

            rust = pkgs.rust-bin.stable.${rustVersion}.default.override {
              targets = [ wasmTarget ];
            };

            naersk' = pkgs.callPackage naersk {
              cargo = rust;
              rustc = rust;
            };

            ri-wasm = naersk'.buildPackage {
              src = ./.;
              copyLibs = true;
              CARGO_BUILD_TARGET = wasmTarget;
              cargoBuildOptions = (opts: opts ++ ["--package=social_sensemaker"]);
            };

          in

          pkgs.stdenv.mkDerivation {
            name = "social_sensemaker-happ";
            buildInputs = [
              holonixMain.pkgs.holochainBinaries.hc
            ];
            unpackPhase = "true";
            installPhase = ''
              mkdir $out
              cp ${ri-wasm}/lib/social_sensemaker.wasm $out
              cp ${happs/social_sensemaker/dna.yaml} $out/dna.yaml
              cp ${happs/social_sensemaker/happ.yaml} $out/happ.yaml
              hc dna pack $out
              hc app pack $out
            '';
          };

        packages.rlp-tui =
          let
            rust = pkgs.rust-bin.stable.${rustVersion}.default;

            naersk' = pkgs.callPackage naersk {
              cargo = rust;
              rustc = rust;
            };

          in

          naersk'.buildPackage {
            src = ./.;
            copyLibs = true;
            cargoBuildOptions = (opts: opts ++ ["--package=frontend-tui"]);
            buildInputs = with pkgs; [
              openssl
              pkgconfig
            ];
          };
      });
}
