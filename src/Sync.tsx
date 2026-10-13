// p3: whether this seat is talking to a team server (the indicator), and — when the person has
// local records and signs into an org — how they join it (the choice).
//
// **Local mode draws nothing.** No account, no server, no indicator: the app is what it always was.

import { useEffect, useState } from "react";
import { api, CLAIMED_BY_ANOTHER, sentence, type JoinChoice, type JoinPlan, type JoinResult } from "./account";
import type { SyncState } from "./bridge";
import { ErrorLine } from "./Forms";
import { countsLine, handleChangesLine, refusalLine } from "./join";
import { agoShort } from "./time";

const WORD: Record<Exclude<SyncState["state"], "local">, string> = {
  online: "Online",
  syncing: "Syncing…",
  offline: "Offline",
  choosing: "Choose how to join",
};

/**
 * Header indicator. **Offline is the state that matters**: writes are refused and nothing is queued,
 * so it is said in the error colour with how long it has been, and it is a `status` the person is
 * told about. Online is quiet — a dot and a word.
 */
export function SyncIndicator({ sync }: { sync: SyncState | undefined }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now()), 5000);
    return () => window.clearInterval(t);
  }, []);
  if (!sync || sync.state === "local") return null;
  const heard = sync.lastSeen != null ? `last heard ${agoShort(sync.lastSeen, now)}` : "not heard from yet";
  const title =
    sync.state === "offline"
      ? `Offline — ${heard}. Reads still work; changes are refused, not queued.`
      : sync.state === "choosing"
        ? "Signed in, but not joined yet — nothing leaves this machine until you choose."
        : `${WORD[sync.state]} — ${heard}`;
  return (
    <div className={`sync sync-${sync.state}`} role="status" title={title} aria-label={`Team sync: ${title}`}>
      <span className="sync-dot" aria-hidden="true" />
      <span>{WORD[sync.state]}</span>
      {sync.state === "offline" ? <span className="sync-heard num">{heard}</span> : null}
    </div>
  );
}

/**
 * The join choice. **The person's, in the window, never automatic** (`sync-contract.md` §7): until
 * they answer, the seat is signed in but not joined, record commands are refused, and nothing
 * leaves the machine. Either way `crm.json` is left exactly as it is — a backup, and what the app
 * returns to if they sign out.
 *
 * `bring` is offered only if the core says so (`plan.choices`): when the local records belong to
 * another account on this machine, the only honest choice is to start from the organisation's.
 */
export function JoinChoice() {
  const [plan, setPlan] = useState<JoinPlan | null>(null);
  const [busy, setBusy] = useState<JoinChoice | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [report, setReport] = useState<JoinResult | null>(null);

  useEffect(() => {
    let live = true;
    api
      .joinPlan()
      .then((p) => live && setPlan(p))
      .catch((e) => live && setError(sentence(e)));
    return () => {
      live = false;
    };
  }, []);

  async function choose(choice: JoinChoice) {
    setBusy(choice);
    setError(null);
    try {
      const r = await api.join(choice);
      // A refusal comes back as a value. Joined: the core flips `seat.sync` and this panel goes;
      // what is left to say is which handles changed.
      if (!r.ok) setError(refusalLine(r));
      else if ((r.handleChanges ?? []).length > 0) setReport(r);
    } catch (e) {
      setError(sentence(e));
    } finally {
      setBusy(null);
    }
  }

  if (report) {
    const changes = report.handleChanges ?? [];
    return (
      <section className="join" aria-label="Your records are in">
        <h2 className="panel-title">Your records are in the organisation</h2>
        <p className="panel-foot">{handleChangesLine(changes)}</p>
        <button type="button" className="composer-open" onClick={() => setReport(null)}>
          Done
        </button>
      </section>
    );
  }

  const canBring = plan ? plan.choices.includes("bring") : false;
  return (
    <section className="join" role="alertdialog" aria-label="Choose how to join">
      <h2 className="panel-title">You have records on this machine, and so may the organisation</h2>
      <dl className="fields join-counts">
        <div className="field">
          <dt className="micro">On this machine</dt>
          <dd>{plan ? countsLine(plan.local) : "…"}</dd>
        </div>
        <div className="field">
          <dt className="micro">In the organisation</dt>
          <dd>{plan ? countsLine(plan.org) : "…"}</dd>
        </div>
      </dl>
      <p className="panel-foot">
        Until you choose, nothing leaves this machine and changes are refused. Either way your file on this machine is left exactly as it
        is — a backup, and what the app returns to if you sign out.
      </p>
      {plan?.reason === CLAIMED_BY_ANOTHER ? (
        <p className="panel-foot join-reason">
          The records on this machine belong to another account, so they cannot be brought into this organisation. You can start from the
          organisation’s records.
        </p>
      ) : null}
      <div className="join-choices">
        {canBring ? (
          <>
            <button type="button" className="composer-open" disabled={busy !== null} onClick={() => choose("bring")}>
              {busy === "bring" ? "Bringing them…" : "Bring my records into the organisation"}
            </button>
            <p className="panel-foot">Uploads what is here, as yours. If a handle is already taken the server picks another, and you are told which.</p>
          </>
        ) : null}
        <button type="button" className="composer-open" disabled={busy !== null || !plan} onClick={() => choose("start-fresh")}>
          {busy === "start-fresh" ? "Joining…" : "Start fresh with the organisation's records"}
        </button>
        <p className="panel-foot">Nothing is uploaded. You see the organisation's records; yours stay in the backup.</p>
      </div>
      {error ? <ErrorLine error={error} onDismiss={() => setError(null)} /> : null}
    </section>
  );
}
