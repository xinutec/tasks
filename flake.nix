# Dev shell for the tasks backend (Rust) + Angular frontend. Enter with: nix develop
# Pure-Rust TLS (rustls) so there's no openssl/pkg-config native dep.
#
# It also exports `packages.task`, the CLI — the half a Claude session uses. That
# is here rather than in the container image because the two halves run in
# different places: the service is a Docker image on isis, and the CLI has to be
# on the PATH of every shell on this Mac, installed through home-manager like
# every other tool. See `pippijn/mac-config`.
{
  description = "tasks — the work Claude sessions and Pippijn hand between each other";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "aarch64-darwin" "x86_64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (s: f nixpkgs.legacyPackages.${s});
    in {
      packages = forAll (pkgs: rec {
        default = task;

        # The `task` CLI, and only it: `--bin task` leaves the server out, which
        # is not a size optimisation but a correctness one — the server is
        # deployed as a container built from `Dockerfile`, and a second way of
        # building it is a second thing to keep in step.
        task = pkgs.rustPlatform.buildRustPackage {
          pname = "task";
          version = "0.1.0";
          # Only what compiling needs. `frontend/` is 200 MB of node_modules on a
          # working machine, and taking the whole tree would rebuild the CLI
          # every time a stylesheet moves.
          src = pkgs.lib.fileset.toSource {
            root = ./.;
            fileset = pkgs.lib.fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              ./src
              # `sqlx::migrate!()` embeds these at compile time. The CLI does not
              # reach that code, but the crate does not build without them.
              ./migrations
            ];
          };
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [ "--bin" "task" ];
          # ⚠ The test suite is not the check here, and pretending otherwise
          # would be worse than skipping it: `tests/tasks_db.rs` needs a real
          # MariaDB, which the build sandbox has no way to supply, and it fails
          # rather than skips *on purpose*. The gate (`nix run ../dev-lint#gate`)
          # supplies one and is what actually gates this repo.
          doCheck = false;
          meta.mainProgram = "task";
        };
      });

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
