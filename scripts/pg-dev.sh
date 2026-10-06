#!/usr/bin/env bash
#
# Sourced, never run: `pg_acquire` leaves DATABASE_URL pointing at a PostgreSQL that is
# safe to write to, and arranges for anything it started to be stopped on exit.
#
# In order of preference:
#   1. DATABASE_URL already set — CI's service container, or the developer's own. Used as
#      is and never touched on the way out.
#   2. Docker — a throwaway `postgres:16` on a loopback port.
#   3. Local PostgreSQL binaries (initdb + pg_ctl) — a throwaway cluster in a temp dir.
#   4. Nothing — stop, and say what to install. Never a silent fallback to "no database".
#
# The throwaway ones listen on 127.0.0.1 only, with a fixed password that protects nothing:
# the database holds nothing but what the tests put there, and dies with the script.
# (A real credential never goes in this file — playbook §13.)

PG_DEV_PORT=${PG_DEV_PORT:-54329}
_pg_started=""   # "docker:<name>" or "local:<dir>" — what we must stop

pg_cleanup() {
  case "$_pg_started" in
    docker:*) docker rm -f "${_pg_started#docker:}" >/dev/null 2>&1 || true ;;
    local:*)  pg_ctl -D "${_pg_started#local:}" -m immediate stop >/dev/null 2>&1 || true
              rm -rf "${_pg_started#local:}" ;;
  esac
  _pg_started=""
}

pg_acquire() {
  if [ -n "${DATABASE_URL:-}" ]; then
    printf '  using DATABASE_URL from the environment\n'
    return 0
  fi
  local url="postgres://postgres:crm-dev@127.0.0.1:$PG_DEV_PORT/postgres"

  if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
    local name="crm-dev-pg-$$"
    docker run -d --rm --name "$name" -e POSTGRES_PASSWORD=crm-dev \
      -p "127.0.0.1:$PG_DEV_PORT:5432" postgres:16 >/dev/null \
      || { printf 'could not start postgres in docker (is port %s taken?)\n' "$PG_DEV_PORT" >&2; return 1; }
    _pg_started="docker:$name"
    local i; for i in $(seq 1 60); do
      docker exec "$name" pg_isready -U postgres >/dev/null 2>&1 && break
      sleep 1
    done
    docker exec "$name" pg_isready -U postgres >/dev/null 2>&1 \
      || { printf 'postgres in docker never became ready\n' >&2; return 1; }
  else
    # Homebrew keeps versioned binaries off PATH.
    local d; for d in /opt/homebrew/opt/postgresql@*/bin /usr/local/opt/postgresql@*/bin /usr/lib/postgresql/*/bin; do
      [ -x "$d/initdb" ] && PATH="$d:$PATH"
    done
    if ! command -v initdb >/dev/null 2>&1 || ! command -v pg_ctl >/dev/null 2>&1; then
      printf 'no PostgreSQL available: start Docker, install PostgreSQL (initdb + pg_ctl on PATH),\nor export DATABASE_URL=postgres://… for one you already run.\n' >&2
      return 1
    fi
    local dir; dir=$(mktemp -d "${TMPDIR:-/tmp}/crm-pg.XXXXXX")
    printf 'crm-dev' > "$dir/.pw"
    initdb -D "$dir/data" -U postgres --auth=password --pwfile="$dir/.pw" >/dev/null \
      || { rm -rf "$dir"; return 1; }
    # The socket lives in the temp dir so nothing collides with a postgres already running.
    pg_ctl -D "$dir/data" -w -l "$dir/log" \
      -o "-p $PG_DEV_PORT -h 127.0.0.1 -k $dir" start >/dev/null \
      || { cat "$dir/log" >&2; rm -rf "$dir"; return 1; }
    _pg_started="local:$dir/data"
    _pg_dir=$dir
  fi
  export DATABASE_URL="$url"
  printf '  throwaway PostgreSQL on 127.0.0.1:%s\n' "$PG_DEV_PORT"
}
