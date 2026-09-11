# Work order — round 2 fixes

**Owners:** backend **and** frontend · **Branches:** `m1-core`, `m3-window`, `m6-brand`
**Source:** [`docs/qa/round-1.md`](../qa/round-1.md)

> **Both agents read this whole file, including the other's section.**
>
> QA found one defect wearing two costumes: the M1 revision added `handle` beside `id`
> additively, and *neither* surface picked it up. Briefed separately, each of you fixes half
> and the other half keeps looking correct in its own harness — which is exactly how this
> survived a revision. The shared rule is stated once, below, and it binds both of you.

---

## The shared rule

**`id` is for machines. `handle` is for humans and agents.**

| | |
|---|---|
| `id` | a ULID. Stored, referenced, keyed on. **Never displayed. Never printed. Never put in a command.** |
| `handle` | `acme`, `acme-2`. Typable, stable across renames. **Everything a person or an agent reads or types.** |

The snapshot carries both, side by side, on every record — `state.rs:844` already does this
for `focus`. If a surface has an `id` and needs to show something, it wants the `handle`.

**The test:** could the person or the agent reading this string type it back into a verb? If
not, it is the wrong field.

---

## Backend — `m1-core`

### 1. MAJOR — `crm status` prints a ULID nobody can type

`src-tauri/src/cli.rs:126-128` · `handle` appears **zero** times in that file; `focus_json`
has been handing it over since the revision.

```
want   looking at: deal acme-renewal
got    looking at: deal 01K4Z8QH3M7XC9VBN2RTFA6EDS
```

Read `focus.handle`. Audit the rest of the file the same way — any place an id reaches
stdout is this bug.

### 2. MAJOR — the test that should have caught it pins the old model

`src-tauri/src/cli.rs:388` passes `"id": "acme"`. Before the revision `acme` *was* an id;
after it, it is a handle. The fixture was never updated, so the test is green and verifies a
model the code no longer has.

Update the fixture to the real post-revision shape — a ULID in `id`, a handle in `handle` —
and assert the output contains the **handle** and **not** the id. Then sweep `state_tests.rs`
for the same staleness: any fixture using a readable string as an `id` is suspect.

### 3. MINOR — a declared-but-unbuilt verb exits 2; the manual says 1

**Decision: make it exit 1.** Exit 2 means *the command line was wrong*; a declared verb that
has not landed yet is a valid request that was refused, which is what the manual's `1` already
covers. A typo keeps exit 2. After M2 this case disappears entirely, but an agent branching on
the code should not have to wait for that.

### 4. MINOR — every argument after the verb is silently discarded

`cli.rs:51` reads `args.first()` and drops the rest, so `crm status --json` and
`crm close --please` both exit 0. Nothing can produce the `exit 2` the manual documents.

You do not need the full grammar here — that is M2 — but **stop silently swallowing**: an
unexpected argument to a no-argument verb exits 2 and says so. The dispatch shape freezes
here, and M2 is built on it.

### 5. Noted — `local_today` is UTC

**Decision: make it local.** A due date is a human calendar concept; a task due "today" in
Istanbul is not due on UTC's today. Fix it now rather than at M4, where it becomes a reminder
firing on the wrong day — a bug the person experiences as the app being unreliable, which is
the worst kind for a CRM.

---

## Frontend — `m3-window` and `m6-brand`

### 1. MAJOR — the window prints opaque ULIDs inside commands it tells the person to type

`src/Record.tsx:61,81` · `src/Table.tsx` · `src/bridge.ts:142-151`

`Row` has `id` and no `handle`. Add `handle: string` to `Row` and to every type that carries a
record reference, and interpolate the **handle** into every printed command.

Keep keying React on `id` — that part is right, and a rename must still relabel in place.

**Then fix the harness that hid it.** `preview.ts` uses readable ids (`d_hollis`,
`c_acme_hold`), which is why the defect is invisible in preview and broken against the real
core. **Make the fixture ULIDs** — the preview must lie in the direction of the truth, or it
is not a harness, it is a comfort blanket.

### 2. MAJOR — seven hard-coded commands used flags nobody had defined

`Board.tsx:78` · `Record.tsx:61,81` · `Table.tsx:61,65` · `Panels.tsx:32`

You were right to flag this as unsettled in `docs/window.md` §3, and wrong to state it to the
person as fact. **The grammar is now frozen** — see [`m2-cli.md`](m2-cli.md) § The argument
grammar, which M2 is being built against.

The good news: your guesses were close enough that only the handle swap is needed. These are
now correct and may be stated:

```
crm add deal "Northwind renewal" --company northwind
crm task <handle> "call back" --due 2026-09-30
crm log note <handle> "…"
```

Check all seven against that section. Anything not in it does not exist — remove it or raise
it with the PM.

### 3. MINOR — the "25 rows in 900px" acceptance box was my arithmetic, not yours

23 rows fit; the shell does not scroll; the list scrolls inside itself. The intent holds and
the number was wrong when I wrote it. **The acceptance box moves, `pageSize` stays at 25.** No
code change — I have corrected `m3-window.md`.

### 4. MINOR — move the reminders caveat next to the due indicator

It is present and well worded, and it sits diagonally opposite the thing it explains. Somebody
reading "2 overdue" in the top-right is not looking at the bottom-left of the rail. Put it
where the eye already is.

### 5. MINOR — `brand.md` publishes a palette the chrome does not use

QA is right that the **darker pair that shipped is correct**: `#2E8B72` is 4.16:1 on white and
`#C08A2E` is 3.04:1 — both fail AA — against 6.38:1 and 4.99:1 for what is in `styles.css`.

The defect is only that `brand.md` never says the chrome uses darker variants or why. Add the
paragraph, with the measured ratios. **Do not change `styles.css`** — the accessible values win.

---

## Acceptance

- [ ] no id reaches stdout, a rendered string, or a printed command on either surface
- [ ] a test on each side fails if one ever does again
- [ ] `preview.ts` fixtures use ULIDs
- [ ] every command the window prints appears in [`m2-cli.md`](m2-cli.md) § the grammar
- [ ] stale `id`-shaped fixtures swept from `state_tests.rs` and `cli.rs`
- [ ] `cargo test` compiles and the count is reported; `npm run verify` green
- [ ] neither agent edited the other's tree

## Do not

- **Do not change the snapshot non-additively.** Both surfaces depend on it.
- **Do not weaken an assertion to make a test pass.** A test that now fails is telling you
  the revision reached further than you thought — bring it to the PM.
- **Backend touches `src-tauri/` only. Frontend touches `src/`, `assets/`, `docs/brand.md`
  only.**
