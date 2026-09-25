// The three pages that wait on the platform — and say so, in one true sentence each.
//
// Not a spinner, not a mock chart, not lorem. A person who clicks Reports should learn
// something true. What they wait for is on `docs/m12-snapshot-gaps.md` and `p0-platform-spec.md`.

import type { Theme } from "./theme";
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

export function Team() {
  return (
    <div className="page stub">
      <p className="stub-line">
        Team lists the people and agents who share a pipeline; here there is one person, and the agents bound to them are on the desk.
      </p>
    </div>
  );
}

const THEMES: [Theme, string, (p: { size?: number }) => JSX.Element][] = [
  ["system", "System", SystemIcon],
  ["light", "Light", SunIcon],
  ["dark", "Dark", MoonIcon],
];

export function Settings({ theme, chooseTheme }: { theme: Theme; chooseTheme: (t: Theme) => void }) {
  return (
    <div className="page stub">
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
      <p className="stub-line">Users, roles and pipeline editing arrive with the platform.</p>
    </div>
  );
}
