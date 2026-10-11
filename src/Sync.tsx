// p3: whether this seat is talking to a team server (the indicator), and — when the person has
// local records and signs into an org — how they join it (the choice).
//
// **Local mode draws nothing.** No account, no server, no indicator: the app is what it always was.

import { useEffect, useState } from "react";
import { api, sentence, type JoinPlan, type JoinChoice } from "./account";
import type { SyncState } from "./bridge";
import { ErrorLine } from "./Forms";
import { countsLine } from "./join";
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
 */
export function JoinChoice() {
  const [plan, setPlan] = useState<JoinPlan | null>(null);
  const [busy, setBusy] = useState<JoinChoice | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [renamed, setRenamed] = useState<{ from: string; to: string }[] | null>(null);

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
      const result = await api.join(choice);
      if (choice === "bring" && result?.renamed?.length) setRenamed(result.renamed);
    } catch (e) {
      setError(sentence(e));
    } finally {
      setBusy(null);
    }
  }

  if (renamed) {
    return (
      <section className="join" aria-label="Your records are in">
        <h2 className="panel-title">Your records are in the organisation</h2>
        <p className="panel-foot">
          {renamed.length} handle{renamed.length === 1 ? "" : "s"} changed so they stay unique: {renamed.map((r) => `${r.from} → ${r.to}`).join(", ")}.
        </p>
        <button type="button" className="composer-open" onClick={() => setRenamed(null)}>
          Done
        </button>
      </section>
    );
  }

  return (
    <section className="join" role="alertdialog" aria-label="Choose how to join">
      <h2 className="panel-title">You have records on this machine, and so may the organisation</h2>
      <dl className="fields join-counts">
        <div className="field">
          <dt className="micro">On this machine</dt>
          <dd>{plan ? countsLine(plan.localCounts) : "…"}</dd>
        </div>
        <div className="field">
          <dt className="micro">In the organisation</dt>
          <dd>{plan ? countsLine(plan.orgCounts) : "…"}</dd>
        </div>
      </dl>
      <p className="panel-foot">
        Until you choose, nothing leaves this machine and changes are refused. Either way your file on this machine is left exactly as it
        is — a backup, and what the app returns to if you sign out.
      </p>
      <div className="join-choices">
        <button type="button" className="composer-open" disabled={busy !== null || !plan} onClick={() => choose("bring")}>
          {busy === "bring" ? "Bringing them…" : "Bring my records into the organisation"}
        </button>
        <p className="panel-foot">Uploads what is here, as yours. If a handle is already taken the server picks another, and you are told which.</p>
        <button type="button" className="composer-open" disabled={busy !== null || !plan} onClick={() => choose("start-fresh")}>
          {busy === "start-fresh" ? "Joining…" : "Start fresh with the organisation's records"}
        </button>
        <p className="panel-foot">Nothing is uploaded. You see the organisation's records; yours stay in the backup.</p>
      </div>
      {error ? <ErrorLine error={error} onDismiss={() => setError(null)} /> : null}
    </section>
  );
}
