{
  inputs = {
    nh-nix-env.url = "github:neighbour-hoods/nh-nix-env";
  };

  outputs = { nh-nix-env, ... }:
    let
      flake-utils = nh-nix-env.metavalues.flake-utils;
      nh-supported-systems = nh-nix-env.metavalues.nh-supported-systems;
      rustVersion = nh-nix-env.metavalues.rustVersion;
      naersk = nh-nix-env.metavalues.naersk;
      wasmTarget = nh-nix-env.metavalues.wasmTarget;
      holonixMain = nh-nix-env.metavalues.holonixMain;
    in
    flake-utils.lib.eachSystem nh-supported-systems (system:
      let
        pkgs = nh-nix-env.values.pkgs;
      in

      {

        devShell = nh-nix-env.packages.${system}.holochainDevShell;

        # packages.social_sensemaker-cargo2nix =
        #   let
        #     # create nixpkgs that contains rustBuilder from cargo2nix overlay
        #     crossPkgs = import nixpkgs {
        #       inherit system;

        #       crossSystem = {
        #         config = "wasm32-unknown-wasi";
        #         system = "wasm32-wasi";
        #         useLLVM = true;
        #       };

        #       overlays = [
        #         (import "${cargo2nix}/overlay")
        #         rust-overlay.overlay
        #       ];
        #     };

        #     # create the workspace & dependencies package set
        #     rustPkgs = crossPkgs.rustBuilder.makePackageSet' {
        #       rustChannel = rustVersion;
        #       packageFun = import ./crates/social_sensemaker/Cargo.nix;
        #       target = "wasm32-unknown-unknown";
        #     };

        #   in

        #   rustPkgs.workspace.social_sensemaker {};

        packages.social_sensemaker-naersk =
          let
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
