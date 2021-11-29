{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    holonix = {
      url = "github:holochain/holonix?rev=48a75e79b1713334ab0086767a214e5b1619d38d";
      flake = false;
    };
    rust-overlay.url = "github:oxalica/rust-overlay";
    cargo2nix.url = "github:cargo2nix/cargo2nix/host-platform-build-rs";
    flake-compat = {
      url = "github:edolstra/flake-compat";
      flake = false;
    };
    naersk.url = "github:mhuesch/naersk";
  };

  outputs = { nixpkgs, flake-utils, holonix, rust-overlay, cargo2nix, naersk, ... }:
    flake-utils.lib.eachSystem ["x86_64-linux" "aarch64-linux"] (system:
      let
        holonixMain = import holonix {
          include = {
            holochainBinaries = true;
          };

          holochainVersionId = "custom";

          holochainVersion = {
            rev = "holochain-0.0.109";
            sha256 = "1rwss1y8cd52ccd0875pfpbw6v518vcry3hjc1lja69x2g2x12qb";
            cargoSha256 = "08a72d7nqpakml657z9vla739cbg8y046av4pwisdgj1ykyzyi60";
            bins = {
              holochain = "holochain";
              hc = "hc";
              kitsune-p2p-proxy = "kitsune_p2p/proxy";
            };

            lairKeystoreHashes = {
              sha256 = "12n1h94b1r410lbdg4waj5jsx3rafscnw5qnhz3ky98lkdc1mnl3";
              cargoSha256 = "0axr1b2hc0hhik0vrs6sm412cfndk358grfnax9wv4vdpm8bq33m";
            };
          };
        };

        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlay ];
        };

        rustVersion = "1.54.0";

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
        };

        packages.holonix = holonixMain;

        packages.rep_interchange-cargo2nix =
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
              packageFun = import ./crates/rep_interchange/Cargo.nix;
              target = "wasm32-unknown-unknown";
            };

          in

          rustPkgs.workspace.rep_interchange {};

        packages.rep_interchange-naersk =
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
              cargoBuildOptions = (opts: opts ++ ["--package=rep_interchange"]);
            };

          in

          pkgs.stdenv.mkDerivation {
            name = "rep_interchange-happ";
            buildInputs = [
              holonixMain.pkgs.holochainBinaries.hc
            ];
            unpackPhase = "true";
            installPhase = ''
              mkdir $out
              cp ${ri-wasm}/lib/rep_interchange.wasm $out
              cp ${happs/rep_interchange/dna.yaml} $out/dna.yaml
              cp ${happs/rep_interchange/happ.yaml} $out/happ.yaml
              hc dna pack $out
              hc app pack $out
            '';
          };
      });
}
