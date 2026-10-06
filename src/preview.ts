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
import { bootstrapOf, feedOf, opsBetween, searchOf, tasksOf, updateOf } from "./previewSplit";
import { asId, EMPTY, findIds, type ActivityKind, type ColumnKey, type Kind, type Snapshot } from "./bridge";
import {
  addRecord, addTask, archiveRecord, doneTask, HUMAN, linkRecords, logActivity, moveDeal, NIA, openRecord, repage,
  setField, SCENARIOS, type WriteResult,
} from "./scenarios";

// MARK: - The fake IPC

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
 *  back a data: URI. One agent has a picture and one does not, so both paths are on screen.
 *
 *  **The picture has to look like what a picture looks like.** It used to be a flat teal disc
 *  with a white silhouette — a token, not a photograph — and beside a Won card it invented a
 *  green-agent collision the product does not have: real avatars are whatever the person
 *  supplies, almost never a single flat colour. So this is a soft, noisy, warm portrait stand-in:
 *  an out-of-focus room, shoulders, a head, film grain. Not a real photo (there is none to
 *  license, and shipping one in a harness would be a strange thing to carry), and nothing in
 *  it is teal. */
function fakeAvatar(path: string): string | null {
  if (!path.includes("nia")) return null;
  return portrait();
}

let portraitUri: string | null | undefined;

function portrait(): string | null {
  if (portraitUri !== undefined) return portraitUri;
  const size = 128;
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;
  const g = canvas.getContext("2d");
  if (!g) return (portraitUri = null);

  // A tiny seeded generator, so the picture is the same one every load.
  let seed = 0x9e3779b9;
  const rand = () => {
    seed = (Math.imul(seed ^ (seed >>> 15), 0x85ebca6b) + 0x27d4eb2f) | 0;
    return ((seed >>> 0) % 10_000) / 10_000;
  };

  // The room: warm, and out of focus.
  const wall = g.createRadialGradient(size * 0.3, size * 0.25, 4, size * 0.5, size * 0.5, size * 0.9);
  wall.addColorStop(0, "#e3cfae");
  wall.addColorStop(1, "#8c6c55");
  g.fillStyle = wall;
  g.fillRect(0, 0, size, size);
  g.filter = "blur(7px)";
  for (const [x, y, r, c] of [
    [22, 30, 16, "#f0dcb8"],
    [104, 22, 13, "#b98d63"],
    [112, 78, 18, "#d8c2a0"],
    [14, 88, 15, "#7d6553"],
  ] as const) {
    g.fillStyle = c;
    g.beginPath();
    g.arc(x, y, r, 0, Math.PI * 2);
    g.fill();
  }

  // The person: shoulders, neck, head, hair.
  g.filter = "blur(1.2px)";
  g.fillStyle = "#2c3543";
  g.beginPath();
  g.ellipse(64, 138, 60, 40, 0, 0, Math.PI * 2);
  g.fill();
  g.fillStyle = "#c48d69";
  g.fillRect(53, 84, 22, 24);
  const skin = g.createLinearGradient(40, 30, 90, 96);
  skin.addColorStop(0, "#e2b48d");
  skin.addColorStop(1, "#c8946f");
  g.fillStyle = skin;
  g.beginPath();
  g.ellipse(64, 60, 24, 30, 0, 0, Math.PI * 2);
  g.fill();
  g.fillStyle = "#3a281f";
  g.beginPath();
  g.ellipse(64, 40, 27, 20, 0, Math.PI, Math.PI * 2);
  g.fill();
  g.fillRect(37, 38, 8, 26);
  g.fillRect(83, 38, 8, 26);
  g.filter = "none";

  // Film grain.
  const px = g.getImageData(0, 0, size, size);
  for (let i = 0; i < px.data.length; i += 4) {
    const n = (rand() - 0.5) * 22;
    px.data[i] += n;
    px.data[i + 1] += n;
    px.data[i + 2] += n;
  }
  g.putImageData(px, 0, 0);

  return (portraitUri = canvas.toDataURL("image/jpeg", 0.86));
}

// MARK: - The fake core
//
// It answers the five read/move envelopes frozen in round 3 §5 — ids on the wire, `page`
// 0-based, `n` 1-based, `kind: null` for all — and, since M7, the seven write envelopes
// `m2-cli.md` § The window's envelope adds, mapped to the mock write functions in
// `scenarios.ts`. A refusal from one of those is returned exactly as the real core would
// send it — `{ ok: false, error }`, never wrapped in `set()` — so `bridge.ts`'s `write()`
// sees the same shape it will see once M2 is real, and `useSnapshot`'s own `apply` (which
// silently drops anything with `ok: false`) never gets the chance to eat it.

const core = {
  snapshot: EMPTY,
  rev: 0,

  /** The watermark the fake data is at. A different counter from `rev`, as in the real core. */
  watermark: 0,

  /** Replace the world and push it, the way a write on the core's side would — as the split
   *  shape's `update`: this seat's view, and the row actions the write took. The response
   *  carries that same update under `split` and nothing else, as the real core's does. */
  set(next: Snapshot): Snapshot {
    const prev = this.snapshot;
    this.rev += 1;
    this.snapshot = { ...next, ok: true, rev: this.rev };
    const ops = opsBetween(prev, this.snapshot, this.watermark);
    const update = updateOf(this.snapshot, this.watermark, ops);
    this.watermark += ops.length;
    void emit("state", update);
    return { ok: true, split: update } as unknown as Snapshot;
  },

  /** The answer to a read: the update for the world as it stands, with no row action. */
  answer(): Snapshot {
    return { ok: true, split: updateOf(this.snapshot, this.watermark, []) } as unknown as Snapshot;
  },

  /** Apply a `WriteResult` from `scenarios.ts`: push and return the snapshot on success,
   *  or hand back the refusal untouched — no `rev`, no push, exactly as a real refusal
   *  never reaches the snapshot at all. */
  resolve(result: WriteResult): Snapshot | { ok: false; error: string } {
    if ("error" in result) return { ok: false, error: result.error };
    return this.set(result.snapshot);
  },

  command(req: Record<string, unknown>): Snapshot | { ok: false; error: string } {
    const s = this.snapshot;
    // The harness can make the core refuse any write, with the exact shape and wording the real
    // one sends when the disk says no (`state.rs`). `window.__refuseWrites = true` in the console
    // — QA round 6c reproduced the dropped error this way, and so can anyone.
    const refuse = (window as unknown as { __refuseWrites?: boolean }).__refuseWrites;
    if (refuse && ["move", "select", "resend", "log", "set", "add", "task", "done", "link", "archive"].includes(req.cmd as string)) {
      return { ok: false, error: "not saved — the disk refused it; nothing changed" };
    }
    switch (req.cmd) {
      case "state":
        return this.answer();

      // --- p1's reads: they answer from the world and touch nothing in it ---
      case "bootstrap":
        return { ok: true, bootstrap: bootstrapOf(s, this.watermark) } as unknown as Snapshot;
      case "changes":
        return { ok: true, changes: { stale: true, watermark: this.watermark } } as unknown as Snapshot;
      case "search":
        return { ok: true, search: searchOf(s, String(req.q ?? ""), Number(req.limit ?? 8)) } as unknown as Snapshot;
      case "feed":
        return { ok: true, feed: feedOf(s, req as never) } as unknown as Snapshot;
      case "tasks":
        return { ok: true, tasks: tasksOf(s, Number(req.limit ?? 8)) } as unknown as Snapshot;

      case "show":
        return this.set(openRecord(s, req.kind as Kind, asId(req.id as string)));

      case "move":
        return this.set(moveDeal(s, asId(req.id as string), req.to as ColumnKey, HUMAN));

      case "follow":
        // The person's own control; the real core refuses it from an agent, and it has no CLI verb.
        return this.set({ ...s, follow: req.on !== false });

      case "resend": {
        // The real core: refused with nobody to hear it, otherwise one signal for the last
        // batch still open, restamped. Here that is "the last send is now".
        const r = s.reminders;
        if (s.agents.length === 0) return { ok: false, error: "no agent is connected to send it to — a reminder needs someone to hear it" };
        if (!r || !r.unconfirmed) return { ok: false, error: "nothing to send again — the last reminder's next steps are all done" };
        return this.set({ ...s, reminders: { ...r, lastSignalAt: Date.now(), lastSignalCount: r.unconfirmed } });
      }

      case "select": {
        const chosen = s.pending?.candidates[(req.n as number) - 1];
        if (!chosen) return this.answer();
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

      // --- M7's write envelopes — round-3-snapshot.md §5's table, m2-cli.md's addition ---

      case "add": {
        const fields = (req.fields ?? {}) as Record<string, string | undefined>;
        return this.resolve(addRecord(s, req.kind as Kind, req.name as string, fields));
      }

      case "set":
        return this.resolve(setField(s, asId(req.id as string), req.field as string, req.value as string));

      case "log":
        return this.resolve(logActivity(s, req.kind as ActivityKind, asId(req.id as string), req.body as string));

      case "task":
        return this.resolve(addTask(s, asId(req.id as string), req.what as string, req.due as string));

      case "done":
        return this.resolve(doneTask(s, asId(req.id as string)));

      case "link":
        return this.resolve(linkRecords(s, asId(req.id as string), asId(req.to as string)));

      case "archive":
        return this.resolve(archiveRecord(s, asId(req.id as string), Boolean(req.restore)));

      default:
        return this.answer();
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
    core.rev += 1; // never rewind: the window would rightly drop a stale message
    core.watermark = 0;
    callSeq = 0;
    pageSeq = 0;
    // Golden snapshots go in verbatim except for `rev`, which is the ordering stamp this
    // harness has to own for `useSnapshot` to accept a switch.
    core.snapshot = { ...scenario.build(), ok: true, rev: core.rev };
    // Remount rather than push: a wholesale replacement looks to the board exactly like
    // several cards moving at once. A fresh mount starts with no history, as launching does.
    remount();
    if (key === "session") pending = window.setTimeout(() => playSession(2000), 600);
    const then = scenario.then;
    if (then) pending = window.setTimeout(() => core.set(then.apply(core.snapshot)), then.after);
  }

  load(keys[0]);
}

// MARK: - An agent at work (harness only)
//
// W1's whole point is that an agent can be *watched*. This plays a scripted agent session against
// the fake core the way the real one would reach it: each call is one line in the feed, with its
// outcome, and — with follow on — it moves the focus and names a page, in the core and not by a
// `show` from the window. The refusal's wording is a stand-in; the real one is the core's.

type Spec = {
  verb: string;
  line: string;
  outcome?: "ok" | "refused" | "asked";
  message?: string;
  handle?: string;
  page?: "pipeline" | "inbox" | "deals" | "contacts";
  /** Change the world the way this call would. */
  effect?: (s: Snapshot) => Snapshot;
};

let callSeq = 0;
let pageSeq = 0;

function agentCall(spec: Spec): void {
  let s = core.snapshot;
  const card = spec.handle ? Object.values(s.cards).find((c) => c.handle === spec.handle) : undefined;
  const outcome = spec.outcome ?? "ok";
  // Follow: the core moves the focus to the call's target, without a `show`.
  if (s.follow !== false && card) s = openRecord(s, "deal", card.id);
  if (outcome === "ok" && spec.effect) s = spec.effect(s);
  const entry = {
    seq: ++callSeq, at: Date.now(), actor: NIA, verb: spec.verb, line: spec.line, outcome,
    ...(spec.message ? { message: spec.message } : {}),
    ...(card ? { target: { kind: "deal" as const, id: card.id, handle: card.handle } } : {}),
  };
  const page = s.follow !== false && spec.page ? { name: spec.page, seq: ++pageSeq } : s.page ?? null;
  core.set({ ...s, agentFeed: [entry, ...(s.agentFeed ?? [])].slice(0, 100), page });
}

/** The 10-second test, at demo speed. `window.__session(ms)` runs it with `ms` between calls. */
function playSession(gap: number): void {
  const steps: Spec[] = [
    { verb: "board", line: "board", page: "pipeline" },
    { verb: "show", line: "show hollis-annual", handle: "hollis-annual" },
    { verb: "log", line: 'log note hollis-annual "MSA redlines received"', handle: "hollis-annual", effect: (s) => {
        const r = logActivity(s, "note", s.focused!.row.id, "MSA redlines received");
        if ("error" in r) return s;
        const f = r.snapshot.focused!;
        return { ...r.snapshot, focused: { ...f, timeline: [{ ...f.timeline[0], by: NIA }, ...f.timeline.slice(1)] } };
      } },
    { verb: "task", line: 'task hollis-annual "Send countersigned MSA" --due 2026-10-14', handle: "hollis-annual", effect: (s) => {
        const r = addTask(s, s.focused!.row.id, "Send countersigned MSA", "2026-10-14");
        if ("error" in r) return s;
        const f = r.snapshot.focused!;
        return { ...r.snapshot, focused: { ...f, tasks: f.tasks.map((t, i) => (i === f.tasks.length - 1 ? { ...t, by: NIA } : t)) } };
      } },
    { verb: "set", line: "set hollis-annual value 70000", handle: "hollis-annual", effect: (s) => {
        const r = setField(s, s.focused!.row.id, "value", "70000");
        return "error" in r ? s : r.snapshot;
      } },
    { verb: "move", line: "move hollis-annual wonn", handle: "hollis-annual", outcome: "refused",
      message: "crm: “wonn” is not a stage — stages: lead, qualified, proposal, negotiation, won, lost" },
    { verb: "move", line: "move hollis-annual won", handle: "hollis-annual", effect: (s) => moveDeal(s, s.focused!.row.id, "won", NIA) },
    { verb: "due", line: "due --week", page: "inbox" },
    { verb: "find", line: "find ferrous", page: "deals" },
  ];
  steps.forEach((step, i) => window.setTimeout(() => agentCall(step), gap * (i + 1)));
}

(window as unknown as { __session: (ms?: number) => void }).__session = (ms = 10_000) => playSession(ms);

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
