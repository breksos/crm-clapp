#!/usr/bin/env bash
#
# The server's tests, against a real PostgreSQL. Called by `verify` (locally and in CI's
# macOS job) and by CI's `server` job, which brings its own database as a service.
#
# `server/` does not exist until p2-backend lands, so while it is absent this says so
# loudly and stops with success. The moment `server/Cargo.toml` exists it is not optional
# any more: tests must run, and a missing database is a failure, not a skip.
set -euo pipefail

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
# shellcheck source=pg-dev.sh
. scripts/pg-dev.sh
trap pg_cleanup EXIT

manifest="server/Cargo.toml"
if [ ! -f "$manifest" ]; then
  msg="no server/ yet (p2-backend has not landed) — server tests NOT RUN"
  printf '\033[33mskip\033[0m  %s\n' "$msg"
  [ -z "${GITHUB_ACTIONS:-}" ] || printf '::warning title=server tests skipped::%s\n' "$msg"
  exit 0
fi

# Same proof as the app's: a runner with no key resolves every dependency (playbook §8).
cargo fetch --locked --manifest-path "$manifest"

pg_acquire || { printf '\033[31mFAIL\033[0m  server/ exists and its tests need a PostgreSQL\n' >&2; exit 1; }
# Isolation between tests is the server's job (a schema or database per test).
cargo test --manifest-path "$manifest" "$@"
