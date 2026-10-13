//! **p3: the replica's side of `AppState`.** Still pure — no I/O, no clock, no network. What it adds: where this seat
//! stands with the server (for the snapshot), a way to put the org's rows and a batch of its log into the dataset,
//! and a checkpoint so a command can be tried and put back. `docs/sync-contract.md` §5.

use super::split::Op;
use super::*;
use crate::sync::{put_feed, put_row, Bootstrap, ServerOp};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SyncState {
    /// No account, or signed out: the seat's own file, no network.
    #[default]
    Local,
    Online,
    /// Bootstrapping or applying `changes`.
    Syncing,
    /// The server cannot be reached: reads work, writes are refused.
    Offline,
    /// Signed in, not yet joined: the person is choosing how (`join.rs`).
    Choosing,
}

impl SyncState {
    pub fn word(self) -> &'static str {
        match self {
            SyncState::Local => "local",
            SyncState::Online => "online",
            SyncState::Syncing => "syncing",
            SyncState::Offline => "offline",
            SyncState::Choosing => "choosing",
        }
    }
}

/// What the snapshot says about the org and the link to it. Never record data; never leaves this window.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SyncInfo {
    pub state: SyncState,
    pub last_seen: Option<i64>,
    pub members: Vec<(String, String)>,
    /// `(id, name, ownerId)`
    pub team_agents: Vec<(String, String, String)>,
}

impl SyncInfo {
    pub fn json(&self) -> Value {
        json!({ "state": self.state.word(), "lastSeen": self.last_seen })
    }

    /// The org's people as `{id, name}`; the seat's own person when the list is empty (local mode).
    pub fn members_json(&self, me: &Person) -> Value {
        if self.members.is_empty() {
            return json!([{ "id": me.id, "name": me.name }]);
        }
        json!(self.members.iter().map(|(id, name)| json!({ "id": id, "name": name })).collect::<Vec<_>>())
    }

    pub fn team_agents_json(&self) -> Value {
        json!(self.team_agents.iter().map(|(id, name, owner)| json!({ "id": id, "name": name, "ownerId": owner })).collect::<Vec<_>>())
    }
}

/// A server op that cannot be applied yet: the log has a hole before it.
#[derive(Debug, PartialEq, Eq)]
pub struct Gap {
    pub have: u64,
}

/// A command's undo: the dataset, and the transient things a command writes outside it.
pub struct Checkpoint {
    db: Db,
    feed: feed::Feed,
    page_hint: Option<feed::PageHint>,
}

impl Checkpoint {
    /// The dataset as it was when the checkpoint was taken.
    pub fn db(&self) -> &Db {
        &self.db
    }
}

impl AppState {
    pub fn checkpoint(&self) -> Checkpoint {
        Checkpoint { db: self.db.clone(), feed: self.feed.clone(), page_hint: self.page_hint.clone() }
    }

    /// Put everything a command touched back, **including** its line in the agent feed.
    pub fn roll_back_command(&mut self, cp: Checkpoint) {
        self.db = cp.db;
        self.feed = cp.feed;
        self.page_hint = cp.page_hint;
    }

    /// Run on a different dataset — the org replica after signing in, the local file after signing out. Everything
    /// about the *seat* (who it is, the roster, the feed, timers) stays; everything about the *data* is the new one's.
    pub fn replace_db(&mut self, db: Db) {
        self.db = db;
        self.log = split::DeltaLog::default();
    }

    /// Put back the **rows** a trial run changed, keeping its line in the agent feed: the write was real as far as the
    /// agent is concerned (it will be told how it ended), but the dataset changes only when the server says yes.
    pub fn roll_back_db(&mut self, cp: Checkpoint) {
        self.db = cp.db;
    }

    /// Put a row the server says is current into the dataset, without moving the watermark.
    pub fn put_current_row(&mut self, kind: &str, row: &Value) -> bool {
        put_row(&mut self.db, kind, row).is_ok()
    }

    /// A person or agent as the seat can name them: from the org's members, the team's agents, or the people this
    /// dataset has seen. Never an id.
    pub fn name_of_actor(&self, actor: &Value) -> String {
        let name_of = |id: &str| -> Option<String> {
            if id == self.me.id {
                return Some(self.me.name.clone());
            }
            self.sync.members.iter().find(|(m, _)| m == id).map(|(_, n)| n.clone()).or_else(|| self.db.people.get(id).cloned())
        };
        match serde_json::from_value::<Actor>(actor.clone()) {
            Ok(Actor::Human { id }) => name_of(&id).unwrap_or_else(|| "a teammate".to_string()),
            Ok(Actor::Agent { id }) => {
                match self.sync.team_agents.iter().find(|(a, _, _)| *a == id) {
                    Some((_, agent, owner)) => match name_of(owner) {
                        Some(o) => format!("{o}'s {agent}"),
                        None => agent.clone(),
                    },
                    None => "a teammate's agent".to_string(),
                }
            }
            Err(_) => "a teammate".to_string(),
        }
    }

    /// Can the seat name this actor? (An unknown one means the team changed since the last bootstrap.)
    pub fn knows_actor(&self, actor: &Value) -> bool {
        match serde_json::from_value::<Actor>(actor.clone()) {
            Ok(Actor::Human { id }) => id.is_empty() || id == self.me.id || self.sync.members.iter().any(|(m, _)| *m == id) || self.db.people.contains_key(&id),
            Ok(Actor::Agent { id }) => self.agents.iter().any(|a| a.id == id) || self.sync.team_agents.iter().any(|(a, _, _)| *a == id),
            Err(_) => true,
        }
    }

    pub fn set_team(&mut self, members: Vec<(String, String)>, agents: Vec<(String, String, String)>) {
        self.sync.members = members;
        self.sync.team_agents = agents;
    }

    /// What a row the server holds says, in a few words — for the sentence a stale refusal ends in.
    pub fn describe_row_now(&self, kind: &str, row: &Value) -> String {
        let text = |k: &str| row.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        match kind {
            "deal" => {
                let value = row.get("value").and_then(|v| serde_json::from_value::<Money>(v.clone()).ok()).map(|m| m.format());
                let state = if text("status") == "open" { text("stage") } else { text("status") };
                match value {
                    Some(v) => format!("its value is now {v} and it is {state}"),
                    None => format!("it has no value and is {state}"),
                }
            }
            "task" => format!("it is due {} — “{}”", text("due"), text("what")),
            _ => {
                format!("it is now called “{}”", text("name"))
            }
        }
    }

    /// The seat's own person is the actor until a command says otherwise.
    pub fn reset_acting(&mut self) {
        self.acting = Actor::Human { id: self.me.id.clone() };
    }

    pub fn acting(&self) -> Actor {
        self.acting.clone()
    }

    pub fn set_sync(&mut self, info: SyncInfo) {
        self.sync = info;
    }

    pub fn sync_info(&self) -> &SyncInfo {
        &self.sync
    }

    pub fn set_sync_state(&mut self, state: SyncState, last_seen: Option<i64>) {
        self.sync.state = state;
        if last_seen.is_some() {
            self.sync.last_seen = last_seen;
        }
    }

    /// The seat's view, for deciding whether a write's view effects still apply (the person may have navigated).
    pub fn view_snapshot(&self) -> View {
        self.db.view.clone()
    }

    /// Give the dataset the view effects a write produced — only if the seat has not looked anywhere else since.
    pub fn adopt_view(&mut self, view: View) {
        self.db.view = view;
    }

    /// Replace every org row with the server's (a first load, or "bootstrap again"). The seat's view, reminders and
    /// marks stay; a task's mark survives if its row does. Returns the dataset's watermark afterwards.
    pub fn load_bootstrap(&mut self, b: &Bootstrap) -> Result<u64, String> {
        let marks: std::collections::HashMap<Id, Option<Timestamp>> = self.db.tasks.iter().map(|t| (t.id.clone(), t.due_sent_at)).collect();
        self.db.companies.clear();
        self.db.contacts.clear();
        self.db.deals.clear();
        self.db.tasks.clear();
        self.db.activities.clear();
        self.db.moves.clear();
        self.db.versions.clear();
        for (kind, row) in &b.rows {
            if crate::sync::ROW_KINDS.contains(&kind.as_str()) {
                put_row(&mut self.db, kind, row)?;
            } else {
                put_feed(&mut self.db, kind, row)?;
            }
        }
        for t in &mut self.db.tasks {
            t.due_sent_at = marks.get(&t.id).copied().flatten();
        }
        if let Some(p) = b.pipeline.get("id").and_then(Value::as_str) {
            self.db.pipelines = vec![Pipeline {
                id: p.to_string(),
                name: b.pipeline.get("name").and_then(Value::as_str).unwrap_or("Sales").to_string(),
                stages: b.pipeline.get("stages").and_then(Value::as_array).map(|a| a.iter().filter_map(|s| Stage::parse(s.as_str()?)).collect()).unwrap_or_else(|| Stage::ALL.to_vec()),
            }];
        }
        self.db.watermark = b.watermark;
        self.sync.members = b.members.clone();
        self.sync.team_agents = b.agents.clone();
        self.log = split::DeltaLog::default(); // a window holding a watermark must bootstrap again
        Ok(self.db.watermark)
    }

    /// Apply a batch of the org's log **in order**. Ops at or below the watermark are skipped (our own writes
    /// come back through here, and that is how they are de-duplicated); a hole is `Err(Gap)` and applies nothing
    /// more. Returns the delta ops to publish (each with the `by` the server logged).
    pub fn apply_server_ops(&mut self, ops: &[ServerOp]) -> Result<Vec<Op>, Gap> {
        let mut out = Vec::new();
        for op in ops {
            if op.seq <= self.db.watermark {
                continue;
            }
            if op.seq != self.db.watermark + 1 {
                return Err(Gap { have: self.db.watermark });
            }
            let applied = if crate::sync::ROW_KINDS.contains(&op.kind.as_str()) {
                // Forward only: a row we already hold at this version or a later one is left alone.
                let held = self.db.versions.get(&op.id).copied().unwrap_or(0);
                let theirs = op.record.get("version").and_then(Value::as_u64).unwrap_or(op.seq);
                if theirs > held { put_row(&mut self.db, &op.kind, &op.record) } else { Ok(()) }
            } else {
                put_feed(&mut self.db, &op.kind, &op.record)
            };
            self.db.watermark = op.seq;
            // A row the core cannot read is skipped, not fatal: the watermark still moves, so one bad row
            // can never wedge the whole replica behind it.
            if applied.is_err() {
                continue;
            }
            if let Some(kind) = kind_word(&op.kind) {
                let by = serde_json::from_value::<Actor>(op.by.clone()).unwrap_or_default();
                out.push(Op { seq: op.seq, kind, id: op.id.clone(), record: self.window_record(&op.kind, &op.id), by });
            }
        }
        Ok(out)
    }

    /// The row as the window's delta carries it (the same shape a local commit's ops have).
    fn window_record(&self, kind: &str, id: &str) -> Value {
        match kind {
            "company" => self.db.company(id).map(split::company_json),
            "contact" => self.db.contact(id).map(split::contact_json),
            "deal" => self.db.deal(id).map(split::deal_json),
            "task" => self.db.tasks.iter().find(|t| t.id == id).map(split::task_json),
            "activity" => self.db.activities.iter().find(|a| a.id == id).map(|a| split::activity_item(&self.db, a)),
            "move" => self.db.moves.iter().find(|m| m.id == id).map(|m| split::move_item(&self.db, m)),
            _ => None,
        }
        .unwrap_or(Value::Null)
    }
}

fn kind_word(k: &str) -> Option<&'static str> {
    Some(match k {
        "company" => "company",
        "contact" => "contact",
        "deal" => "deal",
        "task" => "task",
        "activity" => "activity",
        "move" => "move",
        _ => return None,
    })
}
