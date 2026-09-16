# Work order — round 3: the snapshot the window can actually draw

**Owners:** backend **and** frontend · **Branches:** `m1-core` (backend), `m3-window`
(frontend) · **Source:** [`docs/qa/round-2.md`](../qa/round-2.md)

> **Both agents read this whole file.** Same reason as round 2, and round 2 proved it: the
> defect lives where the surfaces meet, and each side's own harness cannot see it.

`main` now carries the real core — M1 merged at `9220308`, 112 tests. **Rebase on `main`
before you start.**

---

## Why this round exists — the PM's defect, not yours

The snapshot frozen in [`m0-m1-backend.md`](m0-m1-backend.md) gives the board
`columns[].dealIds` and gives `focus` a bare `{ kind, id, handle }`. **From that shape the
window cannot draw a single card title or a single record.** The frontend said so in
`docs/window.md` §1 during M3, with a concrete proposal. It was never settled. The backend was
right not to invent fields it was told not to add, and the window fell back to printing ids —
which is round 2's blocker.

Everything below settles `window.md`'s four open questions. **It is all additive**: nothing
existing is removed, renamed or retyped.

---

## The contract — frozen

### 1. `cards` — the bodies behind `board.columns[].dealIds`

```jsonc
"cards": {
  "<deal id>": {
    …the existing row shape…,                  // kind, id, handle, label, detail, stage, status, value, archived
    "by":      { "kind": "agent", "id": "…" },  // or { "kind": "human" }
    "movedAt": 1757980800000
  }
}
```

One entry per id that appears in any `board.columns[].dealIds`, and no others. The key is an
id — it is an index, nothing reads it.

**`by` is who last put this card where it is.** Not "the actor of the latest activity" as
`window.md` proposed: a stage move is not an activity, so that derivation would name whoever
last logged a *call*, and the ring would tint the wrong agent. Instead:

- **`Deal` gains `moved_by: Actor` and `moved_at: Timestamp`**, set on creation and on every
  `move`. `#[serde(default)]` on both, so existing data files still load.
- The card's `by` and `movedAt` are those fields verbatim.

That is also exactly what the window's ring needs: a card whose `movedAt` changed since the
last snapshot, tinted by `by`.

### 2. `focused` — the record behind `focus`

Present **if and only if** `focus` is non-null.

```jsonc
"focused": {
  "row":           { …the existing row shape… },
  "fields":        [ { "label": "Company", "value": "Hollis Partners" } ],
  "timeline":      [ { "id", "kind", "body", "at", "by" } ],   // newest first
  "timelineTotal": 212,
  "tasks":         [ { "id", "handle", "what", "due", "doneAt", "by" } ]   // open first
}
```

- **`fields` comes from one function in the core, and `crm show` prints the same list in the
  same order.** Two surfaces describing one record two ways is the drift this architecture
  exists to prevent. Labels and formatted values are the core's.
- **`timeline` is capped at the 50 newest**, with `timelineTotal` beside it. A snapshot is
  pushed on every change; an unbounded timeline makes every push as large as the busiest
  record's entire history.
- **Tasks carry a `handle`.** `m2-cli.md` has `crm done <task-handle>` and `Task` has no
  handle today — M2 would have hit this. `Task` gains `handle`, minted from `what` and
  uniquified like every other.

### 3. `list.kind` — the rail's filter is shared state

```jsonc
"list": { …, "kind": "contact" }     // "company" | "contact" | "deal" | null for all
```

People and Companies narrow the **shared** list, for the same reason page and sort are shared:
narrowing only in the window leaves the footer saying "25 of 143" over four visible rows, and
leaves the agent looking at a list the person is not. `crm find --kind` is already in the
grammar.

### 4. Money carries its own formatted string

Every money value on the wire — `row.value`, card values, `board.columns[].totals[]` —
becomes:

```jsonc
{ "amount": 4500000, "currency": "USD", "formatted": "$45,000.00" }
```

`formatted` comes from `Money::format()`. The raw amount stays, because sorting and comparison
need it. **The window deletes its `formatMoney` mirror** and renders `formatted`. One rule, one
implementation.

### 5. The `run_cmd` envelope — frozen as the frontend proposed

`window.md` §3 is adopted as written. It carries **ids, not handles**, and that is right: no
person reads it, and the window already holds the id — a handle would mean resolving a name
nobody asked about.

| envelope | same as |
|---|---|
| `{ cmd: "state" }` | — |
| `{ cmd: "show", kind, id }` | `crm show <handle>` |
| `{ cmd: "move", id, to }` | `crm move <handle> <stage\|won\|lost>` |
| `{ cmd: "select", n }` | `crm select <n>` |
| `{ cmd: "find", query?, sort?, page?, kind? }` | `crm find …` — an omitted field keeps its current value |

`page` in the envelope and in state is **0-based**; everything a person or an agent reads or
types is **1-based**. Same principle as id and handle: the machine form on the wire, the human
form at the edge.

### 6. The golden snapshot — the gate round 2 did not have

This is the structural fix. Round 2's blocker survived because the window was only ever
tested against snapshots the window invented.

- **Backend** adds a test that builds a realistic state through the core's own API — companies,
  deals in several stages, an agent-attributed move, an activity, open and done tasks, a
  record open, and a second state with a `pending` — and writes `AppState::snapshot()` to
  **`src-tauri/fixtures/snapshot.json`** and **`src-tauri/fixtures/snapshot-pending.json`**,
  with real ULIDs.
- The test **fails if the committed files differ from what the core now produces**, with the
  command to regenerate them. A snapshot change is then a visible diff in review, never a
  silent one.
- **Frontend** loads both files verbatim as preview scenarios, and its tests render them. The
  window is then tested against the bytes the core actually emits.

The frontend reads those files and never edits them.

---

## Backend — `m1-core`

1. The snapshot additions in §1–§4, `Deal.moved_by`/`moved_at`, and `Task.handle`, with tests.
2. **`crm show` prints `focused.fields`**, from the same function.
3. **The golden fixtures** in §6.
4. **The envelope in §5**, accepted by `run_cmd` exactly as written, with a test per shape.
5. **MINOR — the clock is read twice** (`main.rs:111-133`), under a comment saying once.
   Derive the local date from `at`. A command at 23:59:59.999 must not carry today's instant
   and tomorrow's date.
6. **MINOR — the local-date tests cannot fail on a UTC machine.** Pull out a pure
   `local_date(at, offset)` and test it with fixed offsets — `+14:00`, `-12:00`, and an instant
   at 23:00 local that is already tomorrow in UTC. CI runs in UTC.

## Frontend — `m3-window`

1. **Consume `cards`, `focused`, `list.kind` and `formatted`.** Delete `formatMoney`.
2. **BLOCKER — never render an id, including as a fallback.** If a card body is missing, draw a
   neutral skeleton, not the id. Type `DealCard`'s `id` as the `Id` brand so the compiler refuses
   to render it. *Degrading to an id* is the same bug as *printing an id*.
3. **Extend the leak guard to rendered text**, and run it over every preview scenario —
   including the two golden fixtures. Round 2's guard only checked command strings.
4. **MAJOR — shell-quote every interpolated value.** One `shellQuote()` for all of
   `commands.ts`: a value matching `[A-Za-z0-9._-]+` stays bare, anything else is POSIX
   single-quoted with `'` written as `'\''`. Single quotes, not double: double quotes still
   expand `$` and backticks. Test with a space, `"`, `'`, `$` and a backtick.
5. **MINOR — the drag payload.** The internal move uses a private type,
   `application/x-breksos-deal`, carrying the id. `text/plain` carries the **handle**, so a card
   dropped on the agent's terminal pastes something it can type back.
6. **MINOR — example commands.** The rule is now in [`m2-cli.md`](m2-cli.md): *a printed
   command that names a record names one that exists.* On the empty board show
   `crm add deal "Northwind renewal"` with no `--company`. When nothing is open, show
   `crm show <handle>` using a real row if there is one; if there are no records at all, show
   `crm add company "Acme Corp"`.

---

## Acceptance

- [ ] the window, fed `fixtures/snapshot.json` verbatim, shows real card titles, the open
      record, its timeline and its tasks — and **zero** ids anywhere in rendered text
- [ ] the leak guard covers rendered text and runs over the golden fixtures
- [ ] the golden test fails when the snapshot changes and the fixture does not
- [ ] `crm show` and the record panel list the same fields in the same order
- [ ] timeline capped at 50 with a total; tasks carry handles
- [ ] every money value carries `formatted`; the window has no formatter of its own
- [ ] `run_cmd` accepts each envelope shape; each has a test
- [ ] every interpolated command value is shell-quoted; the five hostile characters are tested
- [ ] clock read once; the local-date tests fail if UTC is reintroduced, on a UTC machine
- [ ] `cargo test` count reported; `npm test` and `npm run verify` green
- [ ] nothing removed, renamed or retyped in the snapshot

## Sequencing

1. Both start now. The frontend builds against this frozen shape through the preview, as in M3.
2. The backend lands first; the PM reviews and merges it to `main`.
3. **The frontend rebases on `main` before calling itself done** — so its `npm run verify`
   builds the real round-3 core with the real window, and the golden fixtures are the real ones.
4. QA round 3 reviews both together.

## Do not

- **Do not remove, rename or retype anything in the snapshot.**
- **Frontend: do not edit `src-tauri/fixtures/`.** If the fixture is wrong, the core is wrong.
- **Do not build M2 verbs beyond `show`'s field list.** The rest is the next order.
- **Backend touches `src-tauri/` only. Frontend touches `src/`, `preview.html`, `docs/window.md`
  only.**
