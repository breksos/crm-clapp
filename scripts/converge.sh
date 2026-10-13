#!/usr/bin/env bash
#
# The live convergence test (p3 acceptance 1): two real accounts in one org, each with an agent, make 50 mixed writes
# apiece over HTTP at the real server (axum + PostgreSQL), and the server and both replicas must end byte-identical.
#
# Starts a throwaway PostgreSQL (or uses DATABASE_URL), builds and starts `crm-server` on a free loopback port, runs the
# app's `converge` tests with CRM_TEST_SERVER pointing at it, and stops everything it started. The in-memory
# version of the same test runs in every plain `cargo test`; this is the same workload against the real thing.
set -euo pipefail

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
# shellcheck source=pg-dev.sh
. scripts/pg-dev.sh
server_pid=""
cleanup() {
  [ -z "$server_pid" ] || { kill "$server_pid" 2>/dev/null || true; wait "$server_pid" 2>/dev/null || true; }
  pg_cleanup
}
trap cleanup EXIT

pg_acquire
cargo build --manifest-path server/Cargo.toml --bin crm-server
port=${CRM_CONVERGE_PORT:-18787}
BIND="127.0.0.1:$port" "${CARGO_TARGET_DIR:-server/target}/debug/crm-server" 2>/dev/null &
server_pid=$!
for _ in $(seq 1 50); do
  curl -fsS "http://127.0.0.1:$port/healthz" >/dev/null 2>&1 && break
  sleep 0.2
done
curl -fsS "http://127.0.0.1:$port/healthz" >/dev/null || { echo "converge: the server never came up" >&2; exit 1; }

CRM_TEST_SERVER="http://127.0.0.1:$port" cargo test --manifest-path src-tauri/Cargo.toml converge -- --nocapture "$@"
