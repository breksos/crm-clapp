//! **W1 part A: the agent feed.** Every call an agent makes that reaches the app is one line the
//! window can show, as it happens, in order, attributed to that agent — reads and refusals
//! included (`docs/work-orders/w1-watchable.md`).
//!
//! * It is **view state, not data**: it lives only while the app runs and is never persisted, never a
//!   row action, never a signal. The agent is not told about its own calls.
//! * A person's own actions are not entries: only a call that arrives **with a caller id** is.
//! * `line` is rebuilt from the request with every record named by its **handle** — never an id, and
//!   there is nothing secret on a command line to put there.
//! * **Follow** is done here, in the core, on purpose: if the window navigated by sending its own
//!   `show`, the app would emit `deal.opened` to the agent about the agent's own action.

use super::*;
use std::collections::VecDeque;

/// How many calls the feed remembers.
pub const FEED_CAP: usize = 100;

/// Longest a quoted argument is shown, so a pasted paragraph is one readable line.
const ARG_MAX: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    Refused,
    /// It parked a question (`pending`) instead of acting.
    Asked,
}

impl Outcome {
    fn word(self) -> &'static str {
        match self {
            Outcome::Ok => "ok",
            Outcome::Refused => "refused",
            Outcome::Asked => "asked",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub seq: u64,
    pub at: Timestamp,
    pub actor: Actor,
    pub verb: &'static str,
    pub line: String,
    pub outcome: Outcome,
    pub message: Option<String>,
    pub target: Option<(Kind, Id)>,
}

/// Where the window should go after an agent's read. `seq` makes the *same* page twice in a row a
/// new instruction: the window navigates on a change of `seq`, not of `name`.
#[derive(Clone, Debug, PartialEq)]
pub struct PageHint {
    pub name: &'static str,
    pub seq: u64,
}

#[derive(Debug, Default)]
pub struct Feed {
    entries: VecDeque<Entry>,
    seq: u64,
}

impl Feed {
    pub fn seq(&self) -> u64 {
        self.seq
    }
}

/// The `crm -h` verb a request is, or `None` for an envelope that is not an agent's verb (the
/// window's own: `state`, `bootstrap`, `search`, `follow`, …).
fn verb_of(cmd: &str) -> Option<&'static str> {
    Some(match cmd {
        "find" => "find",
        "show" | "open" => "show",
        "board" => "board",
        "stages" => "stages",
        "due" => "due",
        "status" => "status",
        "export" => "export",
        "add" => "add",
        "set" => "set",
        "log" => "log",
        "move" => "move",
        "task" => "task",
        "done" => "done",
        "link" => "link",
        "archive" => "archive",
        "select" => "select",
        "import" => "import",
        _ => return None,
    })
}

/// One shell word: bare if it is plain, otherwise single-quoted (and cut if it is long).
fn word(s: &str) -> String {
    let mut t: String = s.chars().take(ARG_MAX).collect();
    if s.chars().count() > ARG_MAX {
        t.push('…');
    }
    let plain = !t.is_empty() && t.chars().all(|c| c.is_alphanumeric() || "-_.,:/@+=%".contains(c));
    if plain {
        t
    } else {
        format!("'{}'", t.replace('\'', "'\\''"))
    }
}

fn text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

impl AppState {
    /// A record as the agent typed it: its handle. An id (a raw-socket caller's) is looked up and
    /// printed as the handle it names; one that names nothing prints as `?`.
    fn handle_arg(&self, req: &Value, id_key: &str, handle_key: &str) -> String {
        if let Some(h) = req.get(handle_key).and_then(Value::as_str) {
            return word(h);
        }
        match req.get(id_key).and_then(Value::as_str).and_then(|id| self.db.handle_of(id).or_else(|| self.db.tasks.iter().find(|t| t.id == id).map(|t| t.handle.as_str()))) {
            Some(h) => word(h),
            None => "?".to_string(),
        }
    }

    /// The command as the agent typed it, with handles for ids. Built from the request, never
    /// echoed from it, so nothing but what this function names can appear.
    fn describe(&self, verb: &'static str, req: &Value) -> String {
        let mut parts: Vec<String> = vec![verb.to_string()];
        let s = |key: &str| req.get(key).and_then(text);
        match verb {
            "show" => parts.push(self.handle_arg(req, "id", "handle")),
            "find" => {
                if let Some(q) = s("query").filter(|q| !q.is_empty()) {
                    parts.push(word(&q));
                } else if req.get("query").and_then(Value::as_str) == Some("") {
                    parts.push("''".into());
                }
                match req.get("kind") {
                    Some(Value::Null) => parts.extend(["--kind".into(), "all".into()]),
                    Some(Value::String(k)) => parts.extend(["--kind".into(), word(k)]),
                    _ => {}
                }
                if let Some(v) = s("sort") {
                    parts.extend(["--sort".into(), word(&v)]);
                }
                if let Some(p) = req.get("page").and_then(Value::as_u64) {
                    parts.extend(["--page".into(), (p + 1).to_string()]);
                }
                if req.get("archived").and_then(Value::as_bool) == Some(true) {
                    parts.push("--archived".into());
                }
            }
            "board" => {
                if let Some(v) = s("stage") {
                    parts.extend(["--stage".into(), word(&v)]);
                }
            }
            "export" => {
                let plural = match s("kind").as_deref() {
                    Some("company") => "companies",
                    Some("contact") => "contacts",
                    Some("deal") => "deals",
                    _ => "?",
                };
                parts.push(plural.into());
            }
            "add" => {
                let kind = s("kind").unwrap_or_default();
                parts.push(word(&kind));
                if let Some(n) = s("name") {
                    parts.push(word(&n));
                }
                if let Some(f) = req.get("fields").and_then(Value::as_object) {
                    for (key, flag) in [
                        ("company", "--company"), ("domain", "--domain"), ("email", "--email"), ("phone", "--phone"),
                        ("title", "--title"), ("value", "--value"), ("currency", "--currency"), ("stage", "--stage"),
                    ] {
                        if let Some(v) = f.get(key).and_then(text) {
                            // A company named by id on a raw socket still prints as its handle.
                            let v = if key == "company" { self.db.handle_of(&v).map(str::to_string).unwrap_or(v) } else { v };
                            parts.extend([flag.to_string(), word(&v)]);
                        }
                    }
                    for tag in f.get("tags").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                        parts.extend(["--tag".into(), word(tag)]);
                    }
                }
            }
            "set" => {
                parts.push(self.handle_arg(req, "id", "handle"));
                parts.push(word(&s("field").unwrap_or_default()));
                if req.get("clear").and_then(Value::as_bool) == Some(true) {
                    parts.push("--clear".into());
                } else {
                    parts.push(word(&s("value").unwrap_or_default()));
                }
            }
            "log" => {
                parts.push(word(&s("kind").unwrap_or_default()));
                parts.push(self.handle_arg(req, "id", "handle"));
                parts.push(word(&s("body").unwrap_or_default()));
                if let Some(at) = s("at") {
                    parts.extend(["--at".into(), word(&at)]);
                }
            }
            "move" => {
                parts.push(self.handle_arg(req, "id", "handle"));
                parts.push(word(&s("to").unwrap_or_default()));
            }
            "task" => {
                parts.push(self.handle_arg(req, "id", "handle"));
                parts.push(word(&s("what").unwrap_or_default()));
                if let Some(d) = s("due") {
                    parts.extend(["--due".into(), word(&d)]);
                }
            }
            "done" => parts.push(self.handle_arg(req, "id", "handle")),
            "link" => {
                parts.push(self.handle_arg(req, "id", "handle"));
                parts.push(self.handle_arg(req, "toId", "toHandle"));
            }
            "archive" => {
                parts.push(self.handle_arg(req, "id", "handle"));
                if req.get("restore").and_then(Value::as_bool) == Some(true) {
                    parts.push("--restore".into());
                }
            }
            "select" => parts.push(s("n").unwrap_or_default()),
            "import" => {
                let rows = req.get("rows").and_then(Value::as_array).map_or(0, Vec::len);
                parts.push(format!("{}s", s("kind").unwrap_or_default()));
                parts.push(format!("({rows} rows)"));
            }
            _ => {}
        }
        parts.join(" ")
    }

    /// The record a call was about: for a write, the one it named; for `show`/`add`/`select`, the one
    /// they opened. `None` for `stages`, `status`, `export`, `board`, `find`, `due`, `import`, and for
    /// anything that did not resolve to one record.
    fn feed_target(&self, verb: &str, req: &Value, outcome: Outcome) -> Option<(Kind, Id)> {
        if outcome == Outcome::Asked {
            return None;
        }
        let named = |id_key: &str, handle_key: &str| -> Option<(Kind, Id)> {
            if let Some(id) = req.get(id_key).and_then(Value::as_str) {
                return self.kind_of(id).map(|k| (k, id.to_string()));
            }
            let h = req.get(handle_key).and_then(Value::as_str)?;
            self.db.by_handle(h)
        };
        match verb {
            "show" | "add" | "select" if outcome == Outcome::Ok => {
                self.db.view.focus.as_ref().map(|f| (f.kind, f.id.clone()))
            }
            "show" | "add" | "select" => None,
            "set" | "log" | "move" | "task" | "link" | "archive" => named("id", "handle"),
            "done" => {
                let task = match req.get("id").and_then(Value::as_str) {
                    Some(id) => self.db.tasks.iter().find(|t| t.id == id),
                    None => req.get("handle").and_then(Value::as_str).and_then(|h| self.db.task_by_handle(h)),
                }?;
                // A step is about the record it is on — the deal, if it names one.
                let link = task.links.iter().find(|l| self.db.deal(l).is_some()).or_else(|| task.links.first())?;
                self.kind_of(link).map(|k| (k, link.clone()))
            }
            _ => None,
        }
    }

    /// Record one agent call, and — if the seat follows — go where it went. Called by `command`
    /// with the dispatch result **before** the response is built, so the update that answers the
    /// call already carries its own line.
    pub(super) fn record_call(&mut self, req: &Value, caller: &str, ctx: &Ctx, result: &Result<(Answer, Vec<Emit>), String>) {
        let cmd = req.get("cmd").and_then(Value::as_str).unwrap_or("");
        let Some(verb) = verb_of(cmd) else { return };
        let (outcome, message) = match result {
            Ok((Answer::Ambiguous, _)) => (Outcome::Asked, None),
            Ok(_) => (Outcome::Ok, None),
            Err(e) => (Outcome::Refused, Some(e.clone())),
        };
        let target = self.feed_target(verb, req, outcome);
        let line = self.describe(verb, req);
        let actor = self.actor(Some(caller));
        self.feed.seq += 1;
        let seq = self.feed.seq;
        self.feed.entries.push_front(Entry { seq, at: ctx.at(), actor, verb, line, outcome, message, target: target.clone() });
        self.feed.entries.truncate(FEED_CAP);

        if self.db.view.follow {
            if let Some((kind, id)) = &target {
                self.focus_on(*kind, id);
            }
            let page = match verb {
                "board" => Some("pipeline"),
                "due" => Some("inbox"),
                "find" if outcome == Outcome::Ok => Some(self.find_page()),
                // A call about one record lands on the page where that record lives.
                _ => target.as_ref().map(|(kind, _)| match kind {
                    Kind::Deal => "pipeline",
                    _ => "contacts",
                }),
            };
            if let Some(name) = page {
                self.page_hint = Some(PageHint { name, seq });
            }
        }
    }

    /// Which table a `find` is about: the deals, or the contacts-and-companies page.
    fn find_page(&self) -> &'static str {
        let l = &self.db.view.list;
        match l.kind {
            Some(Kind::Deal) => "deals",
            Some(Kind::Contact) | Some(Kind::Company) => "contacts",
            None if l.has_deal_filters() => "deals",
            None => {
                if self.live_ids().iter().any(|id| self.db.deal(id).is_some()) { "deals" } else { "contacts" }
            }
        }
    }

    /// The commit could not reach the disk: the agent was told **refused**, so the feed must say
    /// so too, not "ok". Amends the entry the call just wrote, if there is one.
    pub fn refuse_last_call(&mut self, after_seq: u64, message: &str) {
        if self.feed.seq > after_seq {
            if let Some(e) = self.feed.entries.front_mut() {
                e.outcome = Outcome::Refused;
                e.message = Some(message.to_string());
            }
        }
    }

    pub fn feed_seq(&self) -> u64 {
        self.feed.seq()
    }

    pub(super) fn agent_feed_json(&self) -> Value {
        let rows: Vec<Value> = self
            .feed
            .entries
            .iter()
            .map(|e| {
                let mut v = json!({
                    "seq": e.seq, "at": e.at, "actor": e.actor, "verb": e.verb, "line": e.line,
                    "outcome": e.outcome.word(),
                });
                if let Some(m) = &e.message {
                    v["message"] = json!(m);
                }
                if let Some((kind, id)) = &e.target {
                    v["target"] = json!({ "kind": kind.word(), "id": id, "handle": self.db.handle_of(id) });
                }
                v
            })
            .collect();
        json!(rows)
    }

    pub(super) fn page_json(&self) -> Value {
        match &self.page_hint {
            Some(p) => json!({ "name": p.name, "seq": p.seq }),
            None => Value::Null,
        }
    }

    /// `{cmd:"follow", on}` — the person's choice about their own window. Not an agent's to flip.
    pub(super) fn cmd_follow(&mut self, req: &Value, caller: Option<&str>) -> Result<(Answer, Vec<Emit>), String> {
        if caller.is_some() {
            return Err("following is the person's choice, made in the window".to_string());
        }
        let on = req.get("on").and_then(Value::as_bool).ok_or("follow needs `on`: true or false")?;
        self.db.view.follow = on;
        Ok((Answer::Changed, Vec::new()))
    }
}
