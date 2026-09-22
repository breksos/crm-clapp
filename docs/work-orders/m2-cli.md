# Work order — M2 the CLI

**Owner:** backend · **Branch:** `m2-cli`, from `m1-core` once round 2 lands
**Blocks:** M4, M5, and QA's agent pass

Read [`CLAUDE.md`](../../CLAUDE.md), [`architecture.md`](../architecture.md) §7, and
[`round-2-fixes.md`](round-2-fixes.md) — this order assumes the handle rule is already true.

This is the milestone where the app becomes usable by an agent. Sixteen verbs are declared,
granted, and currently refused; they land here.

---

## The argument grammar — frozen, and the window already prints it

This section is the **surface contract**. The window states these commands to the person as
instructions, so every one of them is a promise. It is frozen; changing it is a PM decision
that changes three things at once (`clatch.json`, `crm -h`, and the window's printed text).

**Every verb takes handles, never ids.** `acme`, `acme-2` — typable, stable across renames.
A ULID never appears in a command line, in help text, or in output.

```
READ
  crm find [<query>] [--kind company|contact|deal|all] [--sort updated|name|value]
           [--page N] [--archived] [-n N]
  crm show <handle>
  crm board [--stage <stage>]
  crm stages
  crm due [--overdue | --today | --week]
  crm status
  crm export <kind> [--format csv|json] [--out <path>]

WRITE
  crm add company <name> [--domain D] [--tag T]...
  crm add contact <name> [--company <handle>] [--email E] [--phone P] [--title T]
  crm add deal <title> [--company <handle>] [--value N] [--currency ISO] [--stage <stage>]
  crm set <handle> <field> <value>
  crm log <call|email|meeting|note> <handle> <body> [--at <date>]
  crm move <handle> <stage | won | lost>
  crm task <handle> <what> --due <date>
  crm done <task-handle>
  crm link <handle> <handle>
  crm archive <handle> [--restore]
  crm select <n>
  crm import <path> [--kind companies|contacts|deals]
```

- **Dates are `YYYY-MM-DD`, interpreted in the person's local timezone.** Not UTC — a task
  due "today" in Istanbul is not due on UTC's today.
- **`find` edits the shared list; an omitted option keeps its current value.** `crm find --page 2`
  turns the page without restating the search, exactly as the window's `find` envelope does
  (round 3 §5). `--kind all` clears the filter; `crm find ""` clears the query. `--archived`
  is not sticky — it applies to that search only.
- **Pages are 1-based at the edge and 0-based in state.** A person or an agent reads and types
  page 1; the wire and `view.list.page` carry 0. Same principle as handle and id.
- **The sort words are the core's `Sort` enum** — `updated`, `name`, `value` today — and
  `crm -h` names them from that enum, never from a second list.
- **`-n N` limits what the terminal prints and nothing else.** It must never touch
  `view.list.page_size`. An agent asking for three results must not repaginate the person's
  table to three rows. A test pins this.
- **`--value` is minor units or a decimal?** Take a decimal (`--value 45000` or `45000.50`)
  and store minor units. Never carry an `f64` into the model.

## The window's envelope

The JSON the window sends on `run_cmd` is frozen in
[`round-3-snapshot.md`](round-3-snapshot.md) §5. It carries ids, not handles — nobody reads
it. M2 accepts every shape there and maps each to the same core call its CLI verb makes.

### The write shapes — added 2026-09-22

Round 3 froze only `state`, `show`, `move`, `select` and `find`. **That left the person unable
to create anything from the window**, which is architecture §10b and the reason
[`m7-window-editing.md`](m7-window-editing.md) exists. M2 accepts these too:

| envelope | same as |
|---|---|
| `{ cmd: "add", kind, name, fields? }` | `crm add <kind> <name> [--flags]` |
| `{ cmd: "set", id, field, value }` | `crm set <handle> <field> <value>` |
| `{ cmd: "log", kind, id, body, at? }` | `crm log <kind> <handle> <body> [--at]` |
| `{ cmd: "task", id, what, due }` | `crm task <handle> <what> --due <date>` |
| `{ cmd: "done", id }` | `crm done <task-handle>` |
| `{ cmd: "link", id, to }` | `crm link <handle> <handle>` |
| `{ cmd: "archive", id, restore? }` | `crm archive <handle> [--restore]` |

**One rule decides everything about them:** each maps to the *same core call* its CLI verb
makes. Not a parallel path, not a second validation, not a looser one. If the two ever diverge
the surfaces drift, which is the failure this whole architecture exists to prevent — and a
window path that skipped a rule the CLI enforces would be the worst version of it.

`add` takes a `fields` object rather than a flag list because the window has a form, not a
command line; the core call underneath is the one `crm add` uses. Ids on the wire, as always.

**These are the shapes that make the human half of the signal set reachable.** A window `add`
or `set` emits `record.changed`; a window `log` emits `note.added`. A CLI write emits nothing —
the agent already knows about its own work.

## Exit codes

The manual documents three, and after this milestone they must be exactly true:

| | |
|---|---|
| `0` | the app answered |
| `1` | the app is not running, or the request was refused |
| `2` | the command line was wrong |

A missing required argument, an unknown flag, an unknown verb, an unparseable date: **2**, and
the message names what was wrong *and* what to do instead. A valid request the core declined —
a handle that matches nothing, a stage that does not exist, archiving something already
archived: **1**.

## What every verb owes the agent

`crm -h` is the agent's only manual, and it is generated from the manifest — keep it that way.
Each verb also answers `crm <verb> -h` with its own arguments once they exist.

**Every refusal teaches.** `invalid argument` is a defect. The bar is: what was wrong, and
what to do instead.

```
want   crm: no record matches "acmee" — try `crm find acme`
not    crm: not found
```

**Ambiguity is a state, not an error.** This is the one piece of M1 that no agent can currently
reach, and it is the most interesting thing in the app. `crm log call acme "…"` where two
companies match parks a `pending` in the shared view, prints the candidates, and returns **0** —
it is not a failure, it is a question. Either surface answers it: `crm select 2`, or a click.
Gate on the **margin** between the top two scores; an exact handle match is decisive.

## Signals — M2 wires the write half

Only **human** actions signal; the agent is never told about its own writes. So these fire when
the *window* does the thing, not when the CLI does:

`deal.opened` (buffered) · `stage.changed` · `record.changed` · `note.added` (context)

`task.due` (run) is **M4**, not here. Do not build a timer.

## Tests

Beyond the per-verb cases:

- [ ] `-n 3` does not change `view.list.page_size`
- [ ] every stage name the board draws is accepted by `move` and named by `stages`
- [ ] an exact handle is decisive; a close margin parks `pending`; `select` resolves it
- [ ] a CLI write emits **no** signal; the equivalent window action emits exactly one
- [ ] exit codes: 2 for a bad command line, 1 for a refused-but-valid request
- [ ] `crm export` writes a real file and prints its path
- [ ] dates parse in local time; a date at 23:00 local does not land on the wrong day
- [ ] an empty pipeline exports and prints `0.00`, never `-0.00`

## Acceptance

- [ ] all sixteen verbs implemented and documented; `crm -h` names exactly what works
- [ ] **QA's agent pass is now runnable**: add a company, a contact and a deal, log a call,
      set a next step, move the deal to proposal, close it won, find, open, page, and trip an
      ambiguity — from `crm -h` alone with the source closed
- [ ] the window's printed commands all parse under this grammar, with handles and
      shell-quoted values — and **a printed command that names a record names one that
      exists**. Commands shown in an empty state may only create; they may not reference a
      record the person does not have.
- [ ] every write envelope above is accepted and routes to the **same core call** as its
      CLI verb — a test per shape asserts the two produce identical state
- [ ] a window write emits its signal; the identical CLI write emits none
- [ ] `cargo test` compiles, count reported; `npm run verify` green
- [ ] no id in any output

## Do not

- **Do not change the grammar above** without the PM. The window prints it.
- **Do not build `task.due`, a timer, or any scheduler.** M4.
- **Do not touch `src/`.** The window is the frontend's tree.
- **Do not let `-n` reach page size.**
