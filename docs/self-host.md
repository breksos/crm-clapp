# Self-hosting the Breksos CRM server

Running your team's CRM on a machine you control: the server, its PostgreSQL, how to put it on
the internet safely, how to back it up and upgrade it, and — plainly — what we can and cannot
see. Nothing here requires an account with us, because there isn't one.

You do **not** need any of this to use the app. With no account the app works on its own file,
exactly as before. A server is only for teams who want to share one pipeline.

## What you are running

Two containers, one command:

| | what | reachable from |
|---|---|---|
| `server` | `crm-server`, the only thing the apps talk to | `127.0.0.1:8787` on the host, nothing else |
| `db` | PostgreSQL 16, holding every record, account and session | the `server` container only — its port is not published |

Everything is in [`deploy/`](../deploy): `compose.yaml`, `server.Dockerfile`, `.env.example`.
You need Docker with the Compose plugin (`docker compose version`) and a machine with a gigabyte
or two to spare. Fifty people is a small load.

## 1. Run it

From a checkout of this repository:

```sh
cp deploy/.env.example deploy/.env
```

Put a password in `deploy/.env` — `openssl rand -hex 24` makes a good one (hex, because it goes
into a URL). `deploy/.env` is ignored by git; keep it that way. There is deliberately no default
password: the compose file refuses to start without one.

```sh
docker compose -f deploy/compose.yaml --env-file deploy/.env up -d --build
curl http://127.0.0.1:8787/healthz        # → ok
```

The first build compiles the server and takes a few minutes. The server applies its own database
migrations when it starts, so there is no separate setup step.

In the app, sign up and give it the server's address. The first person to sign up creates the
organisation; everyone after that joins by invite link (an invite link is a credential — treat
it as one).

`http://127.0.0.1:8787` is fine for trying this on one machine. **The app warns on plain HTTP to
anything but this machine, and so should you.** For anyone else, continue.

## 2. Put it on the internet — TLS and the proxy

The server speaks plain HTTP and expects a proxy you control to terminate TLS in front of it.
Two settings in `deploy/.env` tell it so:

```
CRM_REQUIRE_TLS=1     # refuse any request the proxy did not mark as https
CRM_TRUST_PROXY=1     # believe exactly one proxy: yours
```

- `CRM_REQUIRE_TLS=1` makes the server answer only requests carrying `X-Forwarded-Proto: https`.
  **If your proxy does not set that header, every request is refused** — which is the safe way to
  fail, and the first thing to check if the app says it cannot reach the server.
- `CRM_TRUST_PROXY=1` takes the client's address from the last entry of `X-Forwarded-For` — the
  one your proxy appended. Whatever a client wrote further left is never believed. The login rate
  limit is per address, so a wrong value here either blocks your whole office behind one address
  or lets an attacker pick their own. If the server sits behind two proxies, set `2`.

### The live-updates stream must not be buffered

Teammates' changes reach each other over **one long-lived connection per person**
(`GET /v1/sync/stream`, server-sent events), with a heartbeat every 30 seconds. A proxy that
buffers that response holds the updates back until its buffer fills — to the people using the app
it looks exactly like "sync is slow", and nothing is logged anywhere.

The server sends **`X-Accel-Buffering: no`** on that stream, which nginx obeys. Do not rely on it
alone — turn buffering off for the path and keep the connection open past the heartbeat.

**Caddy** (does not buffer by default; this makes it explicit):

```
crm.example.com {
    reverse_proxy 127.0.0.1:8787 {
        flush_interval -1
    }
}
```

Caddy sets `X-Forwarded-Proto` and `X-Forwarded-For` itself and gets the certificate for you.

**nginx** (you must set the forwarding headers yourself):

```nginx
server {
    listen 443 ssl;
    server_name crm.example.com;
    # ssl_certificate / ssl_certificate_key: yours

    location / {
        proxy_pass http://127.0.0.1:8787;
        proxy_http_version 1.1;
        proxy_set_header Connection "";
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header X-Forwarded-For   $proxy_add_x_forwarded_for;
    }

    location /v1/sync/stream {
        proxy_pass http://127.0.0.1:8787;
        proxy_http_version 1.1;
        proxy_set_header Connection "";
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header X-Forwarded-For   $proxy_add_x_forwarded_for;
        proxy_buffering off;          # the header says so too; say it twice
        proxy_cache off;
        proxy_read_timeout 1h;        # must be well past the 30 s heartbeat
    }
}
```

A CDN or load balancer in front of that counts as another proxy: it must not buffer the stream
either, and `CRM_TRUST_PROXY` must count it.

## 3. Back it up

Everything lives in PostgreSQL's volume. A backup is a database dump:

```sh
docker compose -f deploy/compose.yaml --env-file deploy/.env exec -T db \
  pg_dump -U crm -d crm --format=custom > crm-$(date +%F).dump
```

Restore into an empty database (a fresh `up`, before anyone signs in, or after `down -v`):

```sh
docker compose -f deploy/compose.yaml --env-file deploy/.env exec -T db \
  pg_restore -U crm -d crm --clean --if-exists < crm-2026-10-11.dump
```

- **Test a restore** before you need one — on another machine, not on top of production.
- **A dump is everything**: every record, every email address, and the password and token
  *hashes*. Store it as carefully as the database itself, and encrypt it before it leaves the
  machine.
- Copying the volume's files while the database runs is not a backup.
- People's own machines keep a replica of what they can see, but that is not a backup of the org.

## 4. Upgrade it

```sh
git pull
docker compose -f deploy/compose.yaml --env-file deploy/.env up -d --build
```

1. **Back up first** (above). The server migrates its database on start and migrations only go
   forward; to go back, you restore the dump taken before the upgrade and run the older version.
2. **Read the release notes** for the version you are moving to.
3. Upgrade the server before the apps that talk to it, not after.
4. `docker compose … logs server` should show `listening on 0.0.0.0:8787`. If the server cannot
   reach or migrate the database it says so and exits; the database is not touched further.

Upgrading **PostgreSQL itself** to a new major version (16 → 17) is a different job: dump, start a
new volume on the new version, restore. Changing the `postgres:16` tag alone makes the database
refuse to start.

## 5. What we never see

**We — Breksos, the people who write this software — see none of your data.** Specifically, and
checkable in the source:

- **There is nothing to phone home to.** The server has no analytics, no crash reporting, no
  update check and no licence check. It has no HTTP-client library in its build and opens no
  outbound connection except the one to your PostgreSQL.
- **No account with us exists.** Your accounts live in your database.
- **It writes no request log.** The only lines it ever prints are start-up and error messages,
  and none contains a request, a header, a body, a token or a password.
- **The app talks to one server: the one address you give it.**

**You, the operator, see everything.** That is what running the server means, and it is worth
saying out loud to your team:

- Records — companies, contacts, deals, notes, tasks and the activity history — are stored **in
  the clear**. Anyone who can read the database can read them. There is no end-to-end encryption.
- Names and email addresses are stored in the clear.
- **Passwords, session tokens and invite tokens are stored only as hashes** (argon2id for
  passwords, SHA-256 for tokens). A copy of the database does not contain a usable password or
  session. A token is shown once, when it is made.
- **One person's view never reaches the server.** What someone has open, their filters and their
  board are not sent; neither is anything their agent holds. The server learns an agent exists,
  and by what name, so it can be attributed — not what it was asked.

**What this does not protect against:** a compromised machine, a leaked backup, or an operator who
reads the database. Put the server where you would put anything holding your customer list.

## 6. Day to day

```sh
docker compose -f deploy/compose.yaml --env-file deploy/.env ps        # is it up, is it healthy
docker compose -f deploy/compose.yaml --env-file deploy/.env logs -f server
docker compose -f deploy/compose.yaml --env-file deploy/.env restart server
docker compose -f deploy/compose.yaml --env-file deploy/.env down      # stop; data kept
```

`down -v` also deletes the database volume. That is every record you have. There is no undo.

The containers restart themselves after a reboot (`restart: unless-stopped`). The server
container runs as an unprivileged user, with a read-only filesystem and no extra capabilities; it
writes nothing to disk.

## Known limits

- **The server connects to PostgreSQL without TLS.** In this compose file that connection never
  leaves the private network between the two containers, which is fine. Pointing `DATABASE_URL`
  at a database across a network you do not control is not supported yet.
- **The image is built from source on your machine.** We do not publish one yet.
- **One server, one PostgreSQL.** No clustering, no read replicas.
