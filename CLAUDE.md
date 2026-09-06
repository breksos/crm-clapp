# Breksos CRM

A local-first sales pipeline CRM built as a **clapp**: one binary serving a window for the
person and a CLI for their agent, over one shared state.

## Read before you write anything

1. **[`docs/architecture.md`](docs/architecture.md)** — the spec of record for this app.
   Domain model, the CLI surface, signals, milestones, and the decisions with their
   reasoning.
2. **`clappkit/docs/`** — the platform contract, in the submodule:
   [`elements.md`](clappkit/docs/elements.md) (what a clapp is),
   [`format.md`](clappkit/docs/format.md) (the manifest and its limits),
   [`protocol.md`](clappkit/docs/protocol.md) (the control pipe),
   [`playbook.md`](clappkit/docs/playbook.md) (rules learned by getting them wrong — read
   before shipping).

**Precedence:** the contract wins over `docs/architecture.md`, which wins over any code.
Where an implementation disagrees with a document, the implementation is the bug. Never edit
anything under `clappkit/` — it is a shared upstream submodule.

## Layout

```
docs/architecture.md   the spec of record
clappkit/              the SDK, as a submodule — read-only
clatch.json            identity, launch, and the agent-facing surface       ── M0
src-tauri/src/         state.rs (the core), cli.rs (the agent's verbs)      ── M0
src/                   the window: React + TS, bridge.ts is the only seam   ── M0
scripts/               package.sh, verify.sh                               ── M0
```

**M0 has not landed.** Everything above marked M0 is the target layout, not what is on disk
today. The repository currently holds this file, `docs/`, and the submodule.

## Commands

```sh
npm run build            # never bare `cargo build` — without Tauri's custom-protocol
                         # feature you get a white window
npm run verify           # build → package → clatch validate → CLI ⇄ GUI round-trip
cargo test               # `cargo build` cannot see #[cfg(test)]; run this before believing
                         # anything
cargo fetch --locked     # must resolve with no ssh key present
```

The toolchain (Rust, Node, npm, gh, clatch) is pinned onto `PATH` in
`.claude/settings.json`, so it resolves without your knowing where it was installed.

## Rules that are not negotiable

- **`crm -h` is the agent's only manual.** A verb missing from it does not exist. Every verb
  in `connector.commands` must be implemented and documented — `clatch validate` checks the
  manifest and *nothing* checks that the code agrees with it.
- **Only human actions emit signals.** The agent is never told about its own writes; that is
  the loop that makes an app talk to itself.
- **Any enum a surface shows lives in the core.** If the board draws a "Negotiation" column,
  `crm move` takes that word and `crm -h` names it. A test pins it.
- **Pagination and sort are shared state**, never the caller's. `-n` limits what the terminal
  prints; the page is shared.
- **Nothing secret ever enters a snapshot.** Absent by construction, not by redaction.
- **Never commit `pkg/` or `*.clapp`.** Both are derived, both are gitignored.
- **No "ask the agent" button** anywhere in the window. The person reaches their agent
  through Clatch, not through our UI.

## Working here

Branch per milestone (`m1-core`, `m2-cli`, …). QA verifies before anything merges; the PM
owns the surface contract — `clatch.json` + `crm -h` + the snapshot shape are one artifact,
so a change to any of the three is a change to all three and goes through review.

`AppState` is pure state and logic: no I/O, no networking, no platform code. That is what
makes it testable, and it is why the tests are where the rules actually live.
