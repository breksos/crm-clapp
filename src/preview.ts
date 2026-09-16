// The preview harness: the whole window, in a plain browser, with no Rust build and no
// Clatch.
//
//     npm run preview      →  http://localhost:5174/preview.html
//
// **Why this exists.** It is how the window got designed at all, and it is the only place
// some states are reachable on demand: an agent moving a card while you watch, a `pending`
// question, an overfull column, a title full of shell metacharacters, no records at all —
// and, since round 3, **the core's own golden snapshots**, loaded byte for byte from
// `src-tauri/fixtures/`. Those are listed first, because they are the ones that are true.
//
// The worlds themselves live in `scenarios.ts`, which is pure, so `window.test.ts` renders
// exactly the same set. This file is only the harness around them: a fake IPC layer, a bar
// of buttons, and an alarm.
//
// **What it does NOT do is re-implement the bridge.** It installs Tauri's own `mockIPC`
// underneath `@tauri-apps/api`, so `useSnapshot`, `useAsset` and `cmd` are the real ones,
// running their real code — including the `rev` ordering and the listen/unlisten cycle that
// React's StrictMode exercises twice on mount.
//
// Preview-only. `main.tsx` never imports it, and `preview.html` is not an entry in
// `vite build`, so none of it reaches the shipped bundle.

import "./styles.css";
import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { asId, EMPTY, findIds, type ColumnKey, type Kind, type Snapshot } from "./bridge";
import { HUMAN, moveDeal, openRecord, repage, SCENARIOS } from "./scenarios";

// MARK: - The fake IPC

/** Deliver a snapshot on the `state` event — the same name `clappkit::app::STATE_EVENT`
 *  emits, routed through the mock's own event plugin so the real `listen` receives it. */
function pushState(snapshot: Snapshot): void {
  void emit("state", snapshot);
}

function installFakeCore(): void {
  // `shouldMockEvents` makes the mock answer `plugin:event|listen`, `|emit` and `|unlisten`
  // itself — the half a hand-rolled stub gets wrong, because StrictMode mounts twice and the
  // very first thing the window does is tear a listener down again.
  mockIPC(
    (command, args) => {
      const a = (args ?? {}) as Record<string, unknown>;
      switch (command) {
        case "run_cmd":
          return core.command(a.req as Record<string, unknown>);
        case "asset":
          return fakeAvatar(a.path as string);
        default:
          throw new Error(`preview: no fake for "${command}"`);
      }
    },
    { shouldMockEvents: true },
  );
}

/** Roster avatars are absolute paths the webview cannot open; the core reads them and hands
 *  back a data: URI. One agent has a picture and one does not, so both paths are on screen. */
function fakeAvatar(path: string): string | null {
  if (!path.includes("nia")) return null;
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64">` +
    `<rect width="64" height="64" fill="#267369"/>` +
    `<circle cx="32" cy="25" r="12" fill="#F2EFE6"/>` +
    `<path d="M8 64c0-14 11-22 24-22s24 8 24 22z" fill="#F2EFE6"/></svg>`;
  return `data:image/svg+xml;base64,${btoa(svg)}`;
}

// MARK: - The fake core
//
// It answers the five `run_cmd` envelopes frozen in round 3 §5, in the shapes the real core
// accepts: ids on the wire, `page` 0-based, `n` 1-based, `kind: null` for all.

const core = {
  snapshot: EMPTY,
  rev: 0,

  /** Replace the world and push it, the way a write on the core's side would. */
  set(next: Snapshot): Snapshot {
    this.rev += 1;
    this.snapshot = { ...next, ok: true, rev: this.rev };
    pushState(this.snapshot);
    return this.snapshot;
  },

  command(req: Record<string, unknown>): Snapshot {
    const s = this.snapshot;
    switch (req.cmd) {
      case "state":
        return s;

      case "show":
        return this.set(openRecord(s, req.kind as Kind, asId(req.id as string)));

      case "move":
        return this.set(moveDeal(s, asId(req.id as string), req.to as ColumnKey, HUMAN));

      case "select": {
        const chosen = s.pending?.candidates[(req.n as number) - 1];
        if (!chosen) return s;
        return this.set({ ...openRecord(s, chosen.kind, chosen.id), pending: null });
      }

      case "find": {
        // An omitted field keeps its current value, which is what lets the window turn the
        // page without restating the search.
        const list = { ...s.list };
        if (typeof req.query === "string") {
          list.query = req.query;
          list.page = 0;
        }
        if (typeof req.sort === "string") list.sort = req.sort as never;
        if (typeof req.page === "number") list.page = req.page;
        if ("kind" in req) list.kind = req.kind as Kind | null;
        return this.set({ ...s, list: repage(s, list) });
      }

      default:
        return s;
    }
  },
};

// MARK: - The alarm
//
// Anything rendered into the window that is shaped like an id is a bug by construction. The
// fixtures make such a bug visible; this makes sure somebody sees it. It watches every
// settled render — including the ones a pushed snapshot causes — with the same scanner
// `window.test.ts` uses.

function watchForLeakedIds(root: HTMLElement, report: (leaked: string[]) => void): void {
  let queued = 0;
  new MutationObserver(() => {
    // Coalesce: one render is many mutations, and the answer only matters once it settles.
    if (queued) window.clearTimeout(queued);
    queued = window.setTimeout(() => {
      queued = 0;
      report(findIds(root.textContent ?? ""));
    }, 60);
  }).observe(root, { subtree: true, childList: true, characterData: true });
}

// MARK: - The chrome
//
// Plain DOM, deliberately: it is not part of the window and must not be mistaken for it in a
// screenshot.

const THEMES = [
  ["system", "System"],
  ["light", "Light"],
  ["dark", "Dark"],
] as const;

function chrome(remount: () => void): void {
  const bar = document.createElement("div");
  bar.id = "preview-bar";

  const note = document.createElement("p");
  note.id = "preview-note";

  const alarm = document.createElement("p");
  alarm.id = "preview-alarm";
  alarm.hidden = true;

  function group(label: string, items: readonly (readonly [string, string])[], onPick: (k: string) => void, initial: string) {
    const wrap = document.createElement("div");
    wrap.className = "preview-group";
    const title = document.createElement("span");
    title.className = "preview-label";
    title.textContent = label;
    wrap.append(title);
    for (const [key, text] of items) {
      const b = document.createElement("button");
      b.type = "button";
      b.textContent = text;
      b.dataset.key = key;
      b.setAttribute("aria-pressed", String(key === initial));
      b.addEventListener("click", () => {
        for (const sib of wrap.querySelectorAll("button")) sib.setAttribute("aria-pressed", String(sib === b));
        onPick(key);
      });
      wrap.append(b);
    }
    return wrap;
  }

  const keys = Object.keys(SCENARIOS);
  bar.append(
    group("State", keys.map((k) => [k, SCENARIOS[k].label] as const), load, keys[0]),
    group(
      "Theme",
      THEMES,
      (k) => {
        // Exactly what the window's own toggle does: an explicit choice stamps the root, and
        // "system" removes the stamp so `prefers-color-scheme` decides.
        if (k === "system") document.documentElement.removeAttribute("data-theme");
        else document.documentElement.setAttribute("data-theme", k);
        try {
          localStorage.setItem("breksos.theme", k);
        } catch {
          /* a private window is allowed to refuse, and the app must not care */
        }
      },
      (() => {
        try {
          return localStorage.getItem("breksos.theme") ?? "system";
        } catch {
          return "system";
        }
      })(),
    ),
    note,
    alarm,
  );
  document.body.append(bar);

  watchForLeakedIds(document.getElementById("root")!, (leaked) => {
    alarm.hidden = leaked.length === 0;
    if (leaked.length === 0) return;
    alarm.textContent = `id leaked into the window: ${leaked.join(", ")}`;
    // Loud in the console too: a harness nobody is looking at should still fail audibly.
    console.error("[preview] ids must never be rendered — found:", leaked);
  });

  let pending = 0;

  function load(key: string): void {
    const scenario = SCENARIOS[key];
    note.textContent = scenario.note;
    // A scripted write from the scenario being left must not land in the one being opened.
    window.clearTimeout(pending);
    core.rev += 1; // never rewind: `useSnapshot` would rightly drop a stale snapshot
    // Golden snapshots go in verbatim except for `rev`, which is the ordering stamp this
    // harness has to own for `useSnapshot` to accept a switch.
    core.snapshot = { ...scenario.build(), ok: true, rev: core.rev };
    // Remount rather than push: a wholesale replacement looks to the board exactly like
    // several cards moving at once. A fresh mount starts with no history, as launching does.
    remount();
    const then = scenario.then;
    if (then) pending = window.setTimeout(() => core.set(then.apply(core.snapshot)), then.after);
  }

  load(keys[0]);
}

// MARK: - Go

installFakeCore();

void Promise.all([import("react"), import("react-dom/client"), import("./App")]).then(([React, ReactDOM, App]) => {
  const root = ReactDOM.createRoot(document.getElementById("root")!);
  let generation = 0;
  chrome(() => {
    generation += 1;
    root.render(React.createElement(React.StrictMode, null, React.createElement(App.default, { key: generation })));
  });
});
