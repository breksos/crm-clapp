# Staging: the deploy target (written, not rented)

**Status: a plan. Nothing here has been rented, created or run.** The product owner rents the
server when p3 passes QA ([`work-orders/p3-sync.md`](work-orders/p3-sync.md) §4). This document
is everything needed to do that in one sitting, and the decisions it still needs from you.

Staging is the place QA's live two-person test (p3 §5) and the first real team run against a
real server, on the real internet, behind real TLS. It uses the same files a self-hoster uses
([`self-host.md`](self-host.md), [`deploy/`](../deploy)) — so staging is also the proof that those
files work for someone who is not us.

## Decisions

| | Decision | Why |
|---|---|---|
| **Shape** | **One VM** running Docker: the server and PostgreSQL as containers from `deploy/compose.yaml`, and a TLS proxy (Caddy) on the host | The same thing a self-hoster runs. Nothing about staging is special, so nothing about it can hide a self-hosting bug. |
| **PostgreSQL** | **A container, not a managed service** — for staging | The server connects to PostgreSQL **without TLS** (`NoTls` in `server/src/main.rs`). A managed database sits across a network and almost always demands TLS, so using one today means the backend first adds database TLS. A container on the same VM never leaves a private network. See "Managed PostgreSQL" below for when to revisit. |
| **Server processes** | **Exactly one** | See "One process" below. This is a hard rule, not a sizing choice. |
| **TLS** | **Caddy on the host**, certificates from Let's Encrypt | Free, automatic renewal, and it sets the forwarding headers the server insists on. |
| **Image** | Built on the VM from the tagged source; **no registry** | Nothing is published yet (release plan, p3). One less account and one less credential. |

## Host size for 50 seats

**One VM: 2 vCPU, 4 GiB RAM, 40 GB SSD.** Everything below is arithmetic from the code, not a
measurement — no load test has been run, and the first job after QA is to run one (see "Not
yet known").

| what | the number | where it comes from |
|---|---|---|
| Open connections | **50–100** long-lived streams (one per seat, at most 2) | contract §3.5 |
| Stream upkeep | each stream asks the database for its org's watermark **every 5 s** → ~10–20 trivial queries per second at 50–100 streams | `StreamTiming.poll` |
| Database pool | **16** connections | `Pool::builder(..).max_size(16)` |
| Sign-in bursts | argon2id with the crate's defaults, ~19 MiB and tens of milliseconds per hash. Fifty people signing in at 9:00 is a few hundred MiB for a moment | `Argon2::default()`; sessions last 12 h (`SESSION_TTL`), so a morning burst is the realistic worst case |
| Data | tens of thousands of records and a few hundred thousand activities and ops for a busy 50-person org is well under 1 GB | `ops` is kept ≥14 days and ≥50 000 ops (contract §2) |

So: CPU is nearly idle, memory is dominated by PostgreSQL's cache and the sign-in burst, and disk
is small. **4 GiB leaves PostgreSQL room to cache the whole database; 2 GiB would work and then
surprise you.** 40 GB is generous for data and leaves room for Docker images and one local
backup. Pick a provider that lets you resize without rebuilding.

## Managed PostgreSQL or a container?

|  | container on the VM (**chosen for staging**) | managed PostgreSQL |
|---|---|---|
| Works today | **yes** | **no** — needs database TLS in the server first |
| Backups | ours to run (below) | provider's, with point-in-time recovery |
| Monthly cost | included in the VM | typically the largest line item: often as much as the VM again, or more |
| Failure story | VM dies → restore from backup, minutes to an hour | provider fails over |
| Upgrades | a dump-and-restore for a major version | provider's button |

**Revisit for production**, not staging. The question to ask then is not "is managed better" — it
usually is, for a team without an on-call person — but "has the server learned to speak TLS to
its database". That is a backend change; this document does not make it.

## One process

**Run exactly one `crm-server` process. Not two behind a load balancer, not
`docker compose up --scale server=2`, not an autoscaling platform with a maximum above 1.**

The reason is in the code. A write wakes the streams of its org **through an in-process hub**; a
write made through *another* process never reaches this process's hub, and is noticed only when
each open stream next polls the database — every 5 seconds (`StreamTiming.poll`, with that
exact warning in its comment). Two processes therefore don't break sync, they quietly make it
up-to-5-seconds late for half the people, and the per-seat limit of two streams is counted per
process, not per seat. Nothing fails loudly; it just feels broken.

What follows from that:

- **Deploys restart the one process.** `docker compose up -d --build` replaces the container, and
  apps reconnect with backoff (1 s → 30 s) and catch up from the log. Expect a few seconds.
- **No blue/green with overlap.** Stop the old one, then start the new one.
- **Fifty seats do not need a second process.** One is far under its limits. Horizontal scaling is
  a project (a shared notification channel such as PostgreSQL `LISTEN/NOTIFY`), not a setting.

## The TLS proxy

The server speaks plain HTTP and trusts the proxy in front of it. In `deploy/.env`:

```
CRM_REQUIRE_TLS=1
CRM_TRUST_PROXY=1
```

`/etc/caddy/Caddyfile`:

```
staging.example.com {
    # No `encode` here, deliberately: compression buffers, and the responses are small JSON.

    # The live-updates stream: one long-lived response per seat. It must reach the app as it is
    # written, so: no buffering, and no idle timeout shorter than the 30 s heartbeat.
    @stream path /v1/sync/stream
    handle @stream {
        reverse_proxy 127.0.0.1:8787 {
            flush_interval -1
            transport http {
                read_timeout 1h
                dial_timeout 5s
            }
        }
    }

    handle {
        reverse_proxy 127.0.0.1:8787
    }
}
```

Caddy sets `X-Forwarded-Proto` (without which `CRM_REQUIRE_TLS=1` refuses every request) and
`X-Forwarded-For` (which `CRM_TRUST_PROXY=1` reads from the right) on its own, and fetches and
renews the certificate.

The stream block exists so its settings can be read; the check at step 11 is what proves them. If you use nginx instead, use the stream location from [`self-host.md`](self-host.md) §2: `proxy_buffering off`, `proxy_read_timeout 1h`, HTTP/1.1 with `Connection ""`, and the server's own **`X-Accel-Buffering: no`** header, which nginx obeys.

If a CDN or cloud load balancer sits in front, it is a second proxy: it must not buffer
`text/event-stream` either, its idle timeout must exceed 30 s, `CRM_TRUST_PROXY` becomes `2`, and
(see above) it must send traffic to one server only.

## Backups

Three layers, because each one fails differently:

1. **Nightly dump, off the machine.** A cron job runs `pg_dump --format=custom` (the command in
   [`self-host.md`](self-host.md) §3), encrypts it (`age` with a public key; the private key is
   **not** on the VM), and uploads it to object storage in another region or provider.
   Keep 14 daily and 8 weekly dumps. A dump contains every record and email address and the
   credential hashes: the encryption is not optional.
2. **Provider snapshots of the VM disk**, daily. Fast to restore wholesale; not a substitute for
   (1), because a snapshot of a running database is only crash-consistent and lives with the
   provider that failed.
3. **A restore test, monthly**, onto a scratch VM or laptop, ending in "open the app against it
   and see last night's records". A backup nobody has restored is a hope.

The recovery target this gives: lose at most a day of work, be back within an hour. Anything
tighter than that is WAL archiving or a managed database — a production decision.

## Monthly cost

**Indicative figures from public list prices as the author remembers them, in USD, not quotes.**
Check the provider's pricing page on the day; this table exists to size the decision, not to
fix a budget.

| line | what | per month |
|---|---|---|
| VM | 2 vCPU / 4 GiB / 40 GB, a budget European or US provider | **$8–25** (hyperscalers: $30–40) |
| VM backups | provider snapshots, usually ~20% of the VM | $2–5 |
| Object storage | encrypted dumps, tens of GB | $1–3 |
| Domain | ~$12 a year | $1 |
| DNS, TLS, uptime check | registrar DNS, Let's Encrypt, a free monitor | $0 |
| **Total, container database** | | **≈ $12–35** |
| *Managed PostgreSQL instead* | *adds the database's own line — roughly as much as the VM again or more* | *+$15–60* |

The bill that is not in the table is the person: someone has to be told when this is down, and
apply the OS's security updates. Turn on unattended security upgrades (step 5) and an external
uptime check that pages a human (step 13) or the cheap hosting buys you a weekend.

## From zero to running

Allow two hours, most of it waiting for DNS and the first image build.

**Decide first (the product owner):** the provider and region; the domain (`staging.…`); who
receives the "it's down" alert; where the `age` private key lives.

1. **Domain.** Register or choose one. Create an `A` record `staging.example.com` → the VM's
   address once it exists (and `AAAA` if it has IPv6).
2. **VM.** Ubuntu 24.04 LTS, the size above, in the region nearest the team. SSH key only — add
   your public key at creation and disable password login. Enable the provider's daily snapshots.
3. **Firewall** (provider-level, not just `ufw`): allow **443 and 80** from anywhere (80 is for
   certificate issuance), **22** from your own address only. Nothing else — in particular not 8787
   and never 5432. The compose file already binds both to loopback or the private network; the
   firewall is the second lock.
4. **Docker.** Install Docker Engine and the Compose plugin from Docker's own apt repository;
   `docker compose version` should answer.
5. **Updates.** `apt install unattended-upgrades`, and confirm security updates are on.
   `apt install caddy` from Caddy's repository.
6. **Code.** `git clone https://github.com/breksos/crm-clapp.git && cd crm-clapp`, then
   `git checkout <the tag QA passed>` — never a branch.
7. **Secrets.** `cp deploy/.env.example deploy/.env`; set `POSTGRES_PASSWORD` to the output of
   `openssl rand -hex 24`; set `CRM_REQUIRE_TLS=1` and `CRM_TRUST_PROXY=1`. Save the password in
   the team's password manager. `chmod 600 deploy/.env`.
8. **Proxy.** Write the `Caddyfile` above with your hostname; `systemctl reload caddy`.
9. **Start.** `docker compose -f deploy/compose.yaml --env-file deploy/.env up -d --build`.
   The first build takes a few minutes. `docker compose … ps` should show both containers healthy.
10. **First checks, from your own machine:**
    `curl https://staging.example.com/healthz` → `ok`; `curl -I http://staging.example.com` →
    redirects to https; the certificate is valid in a browser; `nc -vz staging.example.com 5432`
    and `… 8787` both **fail** from outside.
11. **The stream check** (this is the one that matters). Create a throwaway account from the app,
    then sign in with `curl` to get a session token and watch the stream:

    ```sh
    TOKEN=$(curl -fsS https://staging.example.com/v1/signin -H 'content-type: application/json' \
      -d '{"email":"…","password":"…","device":"staging-check"}' | jq -r .token)
    curl -N -i -H "authorization: Bearer $TOKEN" https://staging.example.com/v1/sync/stream
    ```

    Required: `content-type: text/event-stream`, **`x-accel-buffering: no`**, no
    `content-encoding`, and an `event: mark` line **arriving about every 30 seconds as it is
    sent** — not all at once when you interrupt. Then make a change from a second seat and watch a
    `mark` arrive within a second or two. Delete the throwaway account's token afterwards (sign
    out). A stream that arrives in a lump is a buffering proxy, not a slow server.
12. **The app.** Point two seats at `https://staging.example.com`, sign up, invite, and run the
    two-person test (p3 §5). The window must not show an "insecure connection" warning.
13. **Watch it.** Add an external uptime check on `/healthz` that alerts a person, and a second on
    the certificate's expiry.
14. **Back it up.** Install the nightly dump job (above), run it once by hand, and **restore that
    dump** onto another machine before you call staging done.
15. **Write down** the VM's address, the snapshot schedule, who holds the keys, and the tag that
    is running — in the team's runbook, not in this repository.

**Tear-down:** `docker compose … down -v` deletes the database. Staging can be rebuilt from step 9;
the thing worth keeping is the backup you restored in step 14.

## Not yet known

- **Real load.** The sizing is arithmetic. After QA, run 50 simulated seats (streams, a write
  burst, a morning of sign-ins) against staging and replace the table above with measurements.
- **Whether the stream survives the provider's network** — some corporate proxies and mobile
  networks buffer or drop idle connections. The app's fallback (poll every 10 s when the stream
  is down) covers it, and QA should test from a hostile network.
- **Database TLS** — see "Managed PostgreSQL".
- **Prices** — see the cost table.
