// The three pages that wait on the platform — and say so, in one true sentence each.
//
// Not a spinner, not a mock chart, not lorem. A person who clicks Reports should learn
// something true. What they wait for is on `docs/m12-snapshot-gaps.md` and `p0-platform-spec.md`.

import type { Theme } from "./theme";
import type { Account } from "./account";
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

export function Team({ account }: { account: Account }) {
  return account.status.signedIn ? <Roster org={account.status.org?.name ?? ""} /> : (
    <div className="page stub">
      <p className="stub-line">
        Team lists the people who share a pipeline. Signed out there is one person, and the agents bound to them are on the desk; sign in under Settings to see an organisation’s members.
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

export function Settings({ theme, chooseTheme, account }: { theme: Theme; chooseTheme: (t: Theme) => void; account: Account }) {
  return (
    <div className="page stub">
      <AccountSettings account={account} />
      <OrganisationSettings account={account} />

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
      <p className="stub-line">Roles and pipeline editing arrive with the platform.</p>
    </div>
  );
}
