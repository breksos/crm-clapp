#!/usr/bin/env bash
#
# `npm run dev:server` — a local server for the app to talk to, on a throwaway database.
#
#   CRM_SERVER_ADDR   where it listens (default 127.0.0.1:8787)
#   DATABASE_URL      use your own PostgreSQL instead of a throwaway one
#
# Loopback and plain HTTP: this is for development. The hosted server is TLS only, and a
# self-hoster on plain HTTP is warned by the server itself (p2-identity §5) — not here.
# Ctrl-C stops the server and the database it started.
set -euo pipefail

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
# shellcheck source=pg-dev.sh
. scripts/pg-dev.sh
trap pg_cleanup EXIT

[ -f server/Cargo.toml ] || { echo "dev-server: no server/ yet — p2-backend has not landed" >&2; exit 1; }

echo "database"
pg_acquire
export CRM_SERVER_ADDR=${CRM_SERVER_ADDR:-127.0.0.1:8787}
printf '\nserver on http://%s  (Ctrl-C to stop)\n\n' "$CRM_SERVER_ADDR"
# Not exec: the trap must still run to stop the database.
cargo run --manifest-path server/Cargo.toml
