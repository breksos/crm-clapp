# Work order — M1 revision

**Owner:** backend · **Branch:** continue on `m1-core` · **Blocks:** M2, and QA's review of
the core

Three items. The first is a defect; the other two are a decision that changed above you.

---

## 1. `cargo test` does not compile — and that is the finding, not the error

```
error[E0716]: temporary value dropped while borrowed
  --> src/state_tests.rs:463:24
     let lead = column(&st.snapshot(now()), "lead");
```

A `let` binding fixes it. **The fix is not the point.**

`cargo build` passes and `cargo test` does not, which means **770 lines of tests in
`state_tests.rs` have never executed.** Not failed — never ran. So every M1 acceptance box
that rested on them is currently asserted rather than verified: the store round-trip,
won-preserves-stage, the `-0.00` guard, attribution, archive behaviour, the snapshot secret
check.

This is the exact trap named in `CLAUDE.md` and in
[`playbook.md`](../../clappkit/docs/playbook.md)'s field notes: *`cargo build` cannot see
`#[cfg(test)]`; test code rots silently while everything looks green.*

**So:** fix it, then run `cargo test` and **report the number of tests that actually ran**.
If any of them fail once they compile, those failures are the real M1 findings and they come
back to the PM before anything else proceeds.

## 2. Ids become ULIDs; the slug becomes a handle

**Why this changed.** Single-user was a v1 scope decision, not a platform limit — clapps make
network calls routinely. Multi-user is now the v2 goal, and name-derived ids do not survive
it: two machines both create "Acme Corp", both mint `acme`, and on sync nothing can tell
whether that is one company or two. That is unresolvable after the fact.

This is the last cheap moment. M2 has not started, so nothing resolves records by slug yet.

```rust
struct Company {
    id:     Id,        // ULID — the true identity, globally unique, creation-ordered
    handle: String,    // "acme" — human- and agent-typable, unique within the workspace
    name:   String,
    …
}
```

- **`id` is a ULID**, and it is what every reference stores — `company_id`, `contact_ids`,
  `links`, `focus`, `dealIds`.
- **`handle` is the old slug**, derived from the name and uniquified the same way
  (`acme`, `acme-2`). Stable across a rename, exactly as before.
- **Resolution happens at the edge.** `crm show acme` matches the handle and resolves to the
  id. An **exact handle match stays decisive** — architecture §6 — so the ambiguity machinery
  only runs when nothing matches exactly. Agent ergonomics do not change at all.

### This is additive for the window — keep it that way

The snapshot already carries ids as **opaque strings**, and the window keys on them without
interpreting them. Swapping the value from `acme` to a ULID is therefore invisible to M3, and
adding `handle` beside it is additive.

**That holds only while nothing renders an id as user-facing text.** Do not start displaying
`id` anywhere, and do not remove or rename an existing snapshot field. If you believe the
shape must change in any non-additive way, stop and raise it — M3 is already built against it.

## 3. Every mutable record carries `origin`

```rust
origin: InstanceId,   // which install wrote this version
updated_at: Timestamp // already present — keep it
```

`InstanceId` is a UUID minted once on first run and stored in the app's data dir. Without an
origin there is no conflict resolution later, only guessing which of two edits was "first" by
a clock that two machines do not share.

**Activities need none of this** — they are append-only, which is already the right shape for
log-based replication. That decision survives unchanged.

**Build no sync.** No server, no network, no accounts. This item exists purely so that
building sync later is a feature rather than a migration.

## Acceptance

- [ ] `cargo test` **compiles**, and the report states how many tests ran
- [ ] any test that fails now that it compiles is reported to the PM, not quietly fixed
- [ ] `cargo clippy --all-targets` clean on our own code
- [ ] ids are ULIDs; `handle` resolves at the edge; `crm show acme` still works
- [ ] a test pins that two records created from the same name get **different ids** and
      **different handles**
- [ ] a test pins that a rename changes neither the id nor the handle
- [ ] `origin` and `updated_at` round-trip through `CrmStore`
- [ ] the snapshot changed **additively only** — no field removed, renamed, or retyped
- [ ] `npm run verify` passes end to end

## Do not

- **Do not build sync, a server, accounts, or any network call.**
- **Do not change the snapshot non-additively.** M3 is built against it.
- **Do not fix a failing test by weakening its assertion.** A test that now fails is telling
  you something; it comes to the PM.
- **Do not touch `clappkit/`, `src/`, or anything outside `src-tauri/`.**
