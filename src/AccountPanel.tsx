// Account and Organisation, in Settings — and the one loud warning (p2).
//
// Sign up, sign in, sign out and accept an invite happen **here and nowhere else**: there is no
// CLI verb for any of it and no envelope an agent could send, by design (`identity-contract.md`).
// Signed out, the app is exactly what it was: local, on this machine, no account. Nothing on this
// page is needed to use it.

import { useEffect, useState } from "react";
import { api, lastServer, sentence, type Account, type Invite, type Person, type Status } from "./account";
import { ErrorLine } from "./Forms";
import { CheckIcon, PlusIcon } from "./icons";

type Mode = "in" | "up" | "invite";

const MODES: [Mode, string][] = [
  ["in", "Sign in"],
  ["up", "Create account"],
  ["invite", "Join with an invite"],
];

/** Plain HTTP to something that is not this machine: anyone on the path can read the password and
 *  the session. Said loudly, everywhere the window is, until it is fixed or signed out. */
export function InsecureBanner({ status }: { status: Status }) {
  if (!status.signedIn || !status.insecure) return null;
  return (
    <p className="insecure-banner" role="alert">
      <b>Not secure.</b> You are signed in to {status.server ?? "this server"} over plain HTTP. Your password and everything you
      send can be read by anyone on the network between here and there. Use an https:// address, or sign out.
    </p>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="acct-field">
      <span className="micro">{label}</span>
      {children}
    </label>
  );
}

export function AccountSettings({ account }: { account: Account }) {
  const { status } = account;
  const [warning, setWarning] = useState<string | null>(null);

  if (!account.known) return null;

  if (status.signedIn) {
    return (
      <section className="settings-group" aria-label="Account">
        <h2 className="panel-title">Account</h2>
        <dl className="fields acct-facts">
          <div className="field">
            <dt className="micro">Name</dt>
            <dd>{status.person?.name}</dd>
          </div>
          <div className="field">
            <dt className="micro">Server</dt>
            <dd className="mono">{status.server}</dd>
          </div>
        </dl>
        <button
          type="button"
          className="composer-open"
          disabled={account.busy}
          onClick={async () => {
            const r = await account.signOut();
            if (r.ok) setWarning(r.warning);
          }}
        >
          Sign out
        </button>
        {account.error ? <ErrorLine error={account.error} onDismiss={account.dismissError} /> : null}
        <p className="panel-foot">
          Your records stay on this machine either way. Signing out returns the app to local mode.
        </p>
      </section>
    );
  }

  return (
    <section className="settings-group" aria-label="Account">
      <h2 className="panel-title">Account</h2>
      {warning ? (
        <p className="write-error" role="alert">
          <span>{warning}</span>
          <button type="button" className="icon-button write-error-dismiss" aria-label="Dismiss" onClick={() => setWarning(null)}>
            ×
          </button>
        </p>
      ) : null}
      <p className="panel-foot acct-local">
        You are not signed in. The app works exactly as it always has, on this machine, with no account.
      </p>
      <SignedOutForms account={account} />
    </section>
  );
}

function SignedOutForms({ account }: { account: Account }) {
  const [mode, setMode] = useState<Mode>("in");
  const [server, setServer] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [name, setName] = useState("");
  const [orgName, setOrgName] = useState("");
  const [invite, setInvite] = useState("");

  // Read after mount so the first render matches the server's, which has no storage.
  useEffect(() => setServer(lastServer()), []);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    const sent = { server: server.trim(), email: email.trim(), password };
    // The password leaves this window's state the moment it is sent, win or lose.
    setPassword("");
    if (mode === "in") await account.signIn(sent);
    else if (mode === "up") await account.signUp({ ...sent, name: name.trim(), orgName: orgName.trim() });
    else await account.acceptInvite({ ...sent, name: name.trim(), invite: invite.trim() });
    setInvite("");
  }

  const ready =
    server.trim() !== "" &&
    email.trim() !== "" &&
    password.length >= 10 &&
    (mode === "in" || name.trim() !== "") &&
    (mode !== "up" || orgName.trim() !== "") &&
    (mode !== "invite" || invite.trim() !== "");

  return (
    <>
    {account.confirm ? <ServerWarning account={account} /> : null}
    <form className="acct-form" onSubmit={submit} autoComplete="on">
      <div className="sorts" role="group" aria-label="What to do">
        {MODES.map(([key, label]) => (
          <button key={key} type="button" className="chip" aria-pressed={mode === key} onClick={() => setMode(key)}>
            {label}
          </button>
        ))}
      </div>

      <Field label="Server">
        <input className="composer-input" type="url" inputMode="url" placeholder="https://crm.example.com" value={server} onChange={(e) => setServer(e.target.value)} autoComplete="url" />
      </Field>
      {mode === "invite" ? (
        <Field label="Invite">
          <input className="composer-input mono" type="text" spellCheck={false} autoComplete="off" value={invite} onChange={(e) => setInvite(e.target.value)} />
        </Field>
      ) : null}
      {mode !== "in" ? (
        <Field label="Your name">
          <input className="composer-input" type="text" autoComplete="name" value={name} onChange={(e) => setName(e.target.value)} />
        </Field>
      ) : null}
      {mode === "up" ? (
        <Field label="Organisation name">
          <input className="composer-input" type="text" autoComplete="organization" value={orgName} onChange={(e) => setOrgName(e.target.value)} />
        </Field>
      ) : null}
      <Field label="Email">
        <input className="composer-input" type="email" autoComplete="email" value={email} onChange={(e) => setEmail(e.target.value)} />
      </Field>
      <Field label={mode === "in" ? "Password" : "Password (10 characters or more)"}>
        <input
          className="composer-input"
          type="password"
          autoComplete={mode === "in" ? "current-password" : "new-password"}
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
      </Field>

      <button type="submit" className="composer-open" disabled={account.busy || !ready}>
        {mode === "in" ? "Sign in" : mode === "up" ? "Create account" : "Join"}
      </button>
      {account.error ? <ErrorLine error={account.error} onDismiss={account.dismissError} /> : null}
    </form>
    </>
  );
}

/**
 * Nothing has been sent. The address is plain HTTP to a stranger, or carries a name and password in
 * front of its host, and the person is told which host the request would really go to before they
 * decide. Cancel is the default; the password is held only until they answer, and dropped on Cancel.
 */
function ServerWarning({ account }: { account: Account }) {
  const c = account.confirm!;
  const what = c.req.kind === "invite" ? "password and invite" : "password";
  return (
    <div className="server-warning" role="alertdialog" aria-label="Check this server before sending">
      <p className="server-warning-title">
        Your {what} would be sent to <b className="mono">{c.host}</b>.
      </p>
      <ul>
        {c.reasons.map((r) => (
          <li key={r}>{r}</li>
        ))}
      </ul>
      <p className="panel-foot">Nothing has been sent. Only continue if {c.host} is exactly the server you mean.</p>
      <div className="sorts">
        <button type="button" className="chip" autoFocus onClick={account.cancelSend} disabled={account.busy}>
          Cancel
        </button>
        <button type="button" className="chip server-warning-go" onClick={() => void account.confirmSend()} disabled={account.busy}>
          Send it anyway
        </button>
      </div>
    </div>
  );
}

/** The organisation: its name, who is in it (names only — roles are p4's), and a way to invite. */
export function OrganisationSettings({ account }: { account: Account }) {
  const { status } = account;
  if (!status.signedIn || !status.org) return null;
  return (
    <section className="settings-group" aria-label="Organisation">
      <h2 className="panel-title">Organisation</h2>
      <dl className="fields acct-facts">
        <div className="field">
          <dt className="micro">Name</dt>
          <dd>{status.org.name}</dd>
        </div>
      </dl>
      <Members />
      <Invites />
    </section>
  );
}

/** Names only. A member list is as fresh as the last time it was asked for; it is not live. */
export function useMembers(): { members: Person[] | null; error: string | null; reload: () => void } {
  const [members, setMembers] = useState<Person[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [n, setN] = useState(0);
  useEffect(() => {
    let live = true;
    api
      .members()
      .then((m) => live && (setMembers(m), setError(null)))
      .catch((e) => live && setError(sentence(e)));
    return () => {
      live = false;
    };
  }, [n]);
  return { members, error, reload: () => setN((x) => x + 1) };
}

export function MemberList({ members }: { members: Person[] }) {
  return (
    <ul className="members">
      {members.map((m) => (
        <li key={m.id}>{m.name}</li>
      ))}
    </ul>
  );
}

function Members() {
  const { members, error } = useMembers();
  return (
    <div className="acct-members">
      <h3 className="micro">Members</h3>
      {members ? <MemberList members={members} /> : <p className="panel-foot">{error ?? "Loading…"}</p>}
    </div>
  );
}

function Invites() {
  const [email, setEmail] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // **Held in memory only, and only until dismissed.** The server keeps the hash; this is the one
  // time anyone can read the token, so it is shown once and dropped.
  const [made, setMade] = useState<Invite | null>(null);
  const [copied, setCopied] = useState(false);

  async function create(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      setMade(await api.createInvite(email.trim() || null));
      setEmail("");
      setCopied(false);
    } catch (err) {
      setError(sentence(err));
    } finally {
      setBusy(false);
    }
  }

  async function copy(token: string) {
    try {
      await navigator.clipboard.writeText(token);
      setCopied(true);
    } catch {
      /* the token is selectable text; copying by hand works */
    }
  }

  return (
    <div className="acct-invites">
      <h3 className="micro">Invite someone</h3>
      {made ? (
        <div className="invite-once" role="status">
          <p className="panel-foot">
            Give this to them now — it is shown once and cannot be shown again. It works once, and expires{" "}
            <time dateTime={new Date(made.expiresAt).toISOString()}>{new Date(made.expiresAt).toLocaleString()}</time>.
          </p>
          <code className="invite-token" data-testid="invite-token">{made.token}</code>
          <div className="sorts">
            <button type="button" className="chip" onClick={() => copy(made.token)}>
              {copied ? <CheckIcon size={12} /> : null}
              {copied ? "Copied" : "Copy"}
            </button>
            <button type="button" className="chip" onClick={() => setMade(null)}>
              Done — hide it
            </button>
          </div>
        </div>
      ) : (
        <form className="acct-form" onSubmit={create}>
          <Field label="Their email (optional — ties the invite to that address)">
            <input className="composer-input" type="email" autoComplete="off" value={email} onChange={(e) => setEmail(e.target.value)} />
          </Field>
          <button type="submit" className="composer-open" disabled={busy}>
            <PlusIcon size={14} />
            Create invite
          </button>
        </form>
      )}
      {error ? <ErrorLine error={error} onDismiss={() => setError(null)} /> : null}
    </div>
  );
}
