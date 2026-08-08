# Dev shell for the tasks backend (Rust) + Angular frontend. Enter with: nix develop
# Pure-Rust TLS (rustls) so there's no openssl/pkg-config native dep.
{
  description = "tasks — the work Claude sessions and Pippijn hand between each other";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "aarch64-darwin" "x86_64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (s: f nixpkgs.legacyPackages.${s});
    in {
      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = [
            pkgs.cargo
            pkgs.rustc
            pkgs.rust-analyzer
            pkgs.rustfmt
            pkgs.clippy
            pkgs.nodejs_24 # Angular 22 frontend (frontend/)
            pkgs.pnpm # the frontend's installer; node ships npm too, ignore it
            # The gate table is written in Dhall and the rendered gate.json is
            # committed; one check re-renders and diffs the two, so the renderer
            # has to be here rather than assumed on the machine.
            pkgs.dhall-json
            # The DB-backed tests need a server to talk to. Same engine as the
            # cluster runs, so a test cannot pass against a dialect prod lacks.
            pkgs.mariadb
          ];
        };
      });
    };
}
