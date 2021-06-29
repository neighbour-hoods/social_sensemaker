let
  sources = import ./nix/sources.nix;
  rust = import ./nix/rust.nix { inherit sources; };
in

{ pkgs ? import sources.nixpkgs {}
}:

pkgs.mkShell {
  buildInputs = with pkgs; [
    nodejs_latest
    rust
  ];
}
