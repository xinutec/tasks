#!/usr/bin/env bash
# Dev server on the Mac: the API plus the built SPA, open on the LAN (no auth —
# SESSION_SECRET unset). View at http://192.168.1.81:8092; the Mac is headless,
# so localhost is useless from anywhere else.
#
# It needs a database. `with-test-db` brings one up for the length of the run and
# tears it down after, which is what you want for poking at the app — a dev
# server holding a permanent database on this machine would be a second copy of
# the work with nothing keeping it honest. Point DATABASE_URL somewhere lasting
# if you want the rows to survive.
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ -n "${DATABASE_URL:-}" ]]; then
  STATIC_DIR="${STATIC_DIR:-frontend/dist/tasks-web/browser}" \
    exec nix develop -c cargo run
fi

exec nix develop -c nix run ../dev-lint#with-test-db -- \
  --database tasks --user tasks --password tasks --port 3322 \
  --url-env DATABASE_URL -- \
  env STATIC_DIR="${STATIC_DIR:-frontend/dist/tasks-web/browser}" \
  cargo run
