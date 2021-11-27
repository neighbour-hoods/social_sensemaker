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
  };

  outputs = { nixpkgs, flake-utils, holonix, rust-overlay, cargo2nix, ... }:
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
        };

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

        packages.rlp =
          let
            target = "wasm32-unknown-unknown";

            # create nixpkgs that contains rustBuilder from cargo2nix overlay
            crossPkgs = import nixpkgs {
              inherit system;

              crossSystem = {
                config = target;
              };
              overlays = [
                (import "${cargo2nix}/overlay")
                rust-overlay.overlay
              ];
            };

            # create the workspace & dependencies package set
            rustPkgs = crossPkgs.rustBuilder.makePackageSet' {
              rustChannel = "1.56.1";
              packageFun = import ./crates/rep_interchange/Cargo.nix;
              inherit target;
            };

          in

          rustPkgs.workspace.rep_interchange {};
      });
}
