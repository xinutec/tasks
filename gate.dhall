{-
tasks/gate.dhall — this repository's commit gate.

Written against the shared schema from the start rather than converted from a
`verify.sh`, so none of the failures that conversion found elsewhere are here to
find: no `|| true`, no `&&` chain reporting one name for eleven things, and no
build assertion pointed at a directory the build does not write.

Two rows carry the weight of this repository and are worth knowing about before
changing either.

**`tests` brings up its own MariaDB.** The SQL here is runtime strings — sqlx's
`query_as`, not the compile-time macros — so running the queries IS the only
check on them, and a renamed column compiles perfectly and fails on the first
request. `with-test-db` starts a throwaway server, exports the URL, runs the
suite and tears it down. Port 3321: fleetwatch's ephemeral server takes 3317,
messages' 3318, coach's 3319 and life's 3320, and the fleet gate can run all
five at once. The tests themselves PANIC rather than skip when the variable is
absent, so a hand-run cannot report green with none of the SQL exercised — the
state `life` sat in for months.

**`ui-check` is not the whole of looking at it.** It measures geometry at phone
width, and geometry missed both defects in this app's first render (see
`e2e/shots.spec.ts`). `pnpm run shots` is the other half and is deliberately NOT
a row here: it asserts nothing, its output is for a person, and a check nobody
reads is worse than none.

The generated `gate.json` is committed; `the table matches its Dhall` re-renders
and diffs it, so running the gate needs no `dhall`.
-}

let G = ../dev-lint/gate/schema.dhall

in  { name = "tasks"
    , checks =
      [ G.Check::{
        , name = "formatting"
        , argv = G.inDevShell [ "cargo", "fmt", "--all", "--check" ]
        , timeout_s = 180
        }
      , G.Check::{
        , name = "clippy"
        , argv =
            G.inDevShell
              [ "cargo", "clippy", "--all-targets", "--", "-D", "warnings" ]
        , timeout_s = 1800
        }
      , {-  `--test-threads=1`: every DB test shares one database and empties it
            first, so they are not written to interleave.
        -}
        G.Check::{
        , name = "tests (against a real MariaDB)"
        , argv =
              G.inDevShell [ "nix", "run", "../dev-lint#with-test-db", "--" ]
            # [ "--database"
              , "tasks"
              , "--user"
              , "tasks"
              , "--password"
              , "tasks"
              , "--port"
              , "3321"
              , "--url-env"
              , "TASKS_TEST_DATABASE_URL"
              , "--"
              , "cargo"
              , "test"
              , "--"
              , "--test-threads=1"
              ]
        , timeout_s = 3600
        }
      , {-  `--frozen-lockfile` is pnpm ci: install exactly pnpm-lock.yaml, or
            fail. The gate has to run from a clean checkout — a fresh clone, or
            the tree the fleetwatch collector runs in — not just a warm dev
            machine.
        -}
        G.Check::{
        , name = "frontend deps match the lockfile"
        , cwd = "frontend"
        , argv = G.inDevShell [ "pnpm", "install", "--frozen-lockfile" ]
        , timeout_s = 900
        }
      , G.Check::{
        , name = "frontend lint"
        , cwd = "frontend"
        , argv = G.inDevShell [ "pnpm", "run", "lint" ]
        , timeout_s = 900
        }
      , {-  `ng build` compiles only what `src/main.ts` imports, and Playwright
            strips types with esbuild rather than checking them — so without this
            row the specs and the fixtures they share would be the least-checked
            code in the repository.
        -}
        G.Check::{
        , name = "frontend typecheck (e2e)"
        , cwd = "frontend"
        , argv = G.inDevShell [ "pnpm", "run", "typecheck" ]
        , timeout_s = 900
        }
      , G.Check::{
        , name = "frontend unit tests"
        , cwd = "frontend"
        , argv = G.inDevShell [ "pnpm", "test" ]
        , env = G.oneAngularWorker
        , timeout_s = 1800
        }
      , {-  `ng`'s own output path is asserted on, and `ngBuild` additionally
            requires that this run REWROTE it — so a row pointed at a stale
            directory fails rather than passing against whatever a previous
            build left there.
        -}
        G.Check::{
        , name = "frontend build"
        , cwd = "frontend"
        , argv = G.ngBuild "../../" [ "dist/tasks-web/browser" ] [ "pnpm", "run", "build" ]
        , timeout_s = 1800
        }
      , G.Check::{
        , name = "frontend ui-check (phone-width layout harness)"
        , cwd = "frontend"
        , argv = G.inDevShell [ "pnpm", "run", "ui-check" ]
        , env = G.oneAngularWorker
        , timeout_s = 1800
        }
      , {-  The CLI is delivered by nix, not by the container, so `cargo build`
            passing says nothing about whether a session can still install it:
            the package pins its own source fileset and its own lockfile, and
            adding a file outside that fileset breaks the build here and nowhere
            else. Cheap after the first run — nix answers from the store unless
            something it depends on actually moved.
        -}
        G.Check::{
        , name = "the CLI still packages"
        , argv = [ "nix", "build", ".#task", "--no-link" ]
        , timeout_s = 1800
        }
      , G.checkTable "../dev-lint"
      , G.devLint "../"
      ]
    }
