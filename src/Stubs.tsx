// The three pages that wait on the platform — and say so, in one true sentence each.
//
// Not a spinner, not a mock chart, not lorem. A person who clicks Reports should learn
// something true. What they wait for is on `docs/m12-snapshot-gaps.md` and `p0-platform-spec.md`.

import type { Theme } from "./theme";
import type { Account } from "./account";
import { showAccount } from "./teamServer";
import { AccountSettings, MemberList, OrganisationSettings, useMembers } from "./AccountPanel";
import { MoonIcon, SunIcon, SystemIcon } from "./icons";

export function Reports() {
  return (
    <div className="page stub">
      <p className="stub-line">
        Reports compare a team’s pipeline over time; this app holds one person’s pipeline and no history to compare, so they arrive with the platform.
      </p>
    </div>
  );
}

export function Team({ account, teamServer = false }: { account: Account; teamServer?: boolean }) {
  if (account.status.signedIn) return <Roster org={account.status.org?.name ?? ""} />;
  return (
    <div className="page stub">
      <p className="stub-line">
        {teamServer
          ? "Team lists the people who share a pipeline. Signed out there is one person, and the agents bound to them are on the desk; sign in under Settings to see an organisation’s members."
          : "Team lists the people and agents who share a pipeline; here there is one person, and the agents bound to them are on the desk."}
      </p>
    </div>
  );
}

/** Names only: who is in the organisation. No performance view yet, and no roles — those are p4's. */
function Roster({ org }: { org: string }) {
  const { members, error } = useMembers();
  return (
    <div className="page stub">
      <section className="settings-group" aria-label="Members">
        <h2 className="panel-title">{org ? `${org} — members` : "Members"}</h2>
        {members ? <MemberList members={members} /> : <p className="panel-foot">{error ?? "Loading…"}</p>}
        <p className="panel-foot">Reports on what each person and agent has done arrive with the platform.</p>
      </section>
    </div>
  );
}

const THEMES: [Theme, string, (p: { size?: number }) => JSX.Element][] = [
  ["system", "System", SystemIcon],
  ["light", "Light", SunIcon],
  ["dark", "Dark", MoonIcon],
];

export function Settings({
  theme,
  chooseTheme,
  account,
  teamServer,
}: {
  theme: Theme;
  chooseTheme: (t: Theme) => void;
  account: Account;
  /** Whether the team server setting is on, and how to change it. Absent: off, and no way to ask. */
  teamServer?: { on: boolean; set: (on: boolean) => void };
}) {
  const signedIn = account.status.signedIn;
  const shown = showAccount(teamServer?.on ?? false, signedIn);
  return (
    <div className="page stub">
      {shown ? <AccountSettings account={account} /> : null}
      {shown ? <OrganisationSettings account={account} /> : null}

      <section className="settings-group" aria-label="Theme">
        <h2 className="panel-title">Theme</h2>
        <div className="sorts" role="group" aria-label="Theme">
          {THEMES.map(([key, label, Glyph]) => (
            <button key={key} type="button" className="chip" aria-pressed={theme === key} onClick={() => chooseTheme(key)}>
              <Glyph size={12} />
              {label}
            </button>
          ))}
        </div>
        <p className="panel-foot">Kept on this device, like a saved view; it never enters the snapshot.</p>
      </section>
      {teamServer ? (
        <section className="settings-group" aria-label="Advanced">
          <h2 className="panel-title">Advanced</h2>
          <label className="follow">
            <input
              type="checkbox"
              checked={shown}
              disabled={signedIn}
              onChange={(e) => teamServer.set(e.target.checked)}
            />
            <span>Team server (self-hosted)</span>
          </label>
          <p className="panel-foot">
            {signedIn
              ? "You are signed in, so this stays on until you sign out."
              : "For teams that host their own. Leave it off to use the app on this machine alone."}
          </p>
        </section>
      ) : null}
      <p className="stub-line">Roles and pipeline editing arrive with the platform.</p>
    </div>
  );
}
