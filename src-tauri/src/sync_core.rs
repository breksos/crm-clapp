//! **p3: `Core` while signed in.** `docs/sync-contract.md` §5.3–§5.7.
//!
//! Three rules shape everything here:
//!
//! 1. **The `Core` lock is never held across a network call.** A write takes it twice, briefly, around the call;
//!    a read or a navigation never waits on one.
//! 2. **The replica moves only along the org's log.** A write the server accepted is applied by fetching the log up
//!    to its `seq` and applying it through the same path a teammate's ops take — so the replica never holds a row
//!    the server did not produce.
//! 3. **Confirmed before acknowledged.** The CLI's exit 0 and the window's success mean the server has it.

use super::*;
use crate::replica::ReplicaStore;
use crate::state::replicate::{SyncInfo, SyncState};
use crate::sync::{row_ops, Backend, Changes, Offline, WriteAnswer};
use std::sync::atomic::{AtomicU64, Ordering};

/// How the window is told something happened that no command of its own caused.
pub type Pusher = Arc<dyn Fn(Value) + Send + Sync>;

/// While the poll fallback runs (the stream is down), how often the app asks for the log.
pub const POLL_EVERY: Duration = Duration::from_secs(10);

/// The store the writer saves through: the local file, or — while signed in and joined — the org replica.
pub struct SwitchStore {
    local: Arc<dyn CrmStore>,
    current: std::sync::RwLock<Arc<dyn CrmStore>>,
}

impl SwitchStore {
    pub fn new(local: Arc<dyn CrmStore>) -> SwitchStore {
        SwitchStore { current: std::sync::RwLock::new(local.clone()), local }
    }
    pub fn use_store(&self, s: Arc<dyn CrmStore>) {
        *self.current.write().unwrap_or_else(|e| e.into_inner()) = s;
    }
    pub fn use_local(&self) {
        self.use_store(self.local.clone());
    }
    fn get(&self) -> Arc<dyn CrmStore> {
        self.current.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl CrmStore for SwitchStore {
    fn load(&self) -> anyhow::Result<model::Db> {
        self.get().load()
    }
    fn save(&self, db: &model::Db) -> anyhow::Result<()> {
        self.get().save(db)
    }
    fn apply(&self, ops: &[crate::sync::ServerOp], watermark: u64) -> anyhow::Result<()> {
        self.get().apply(ops, watermark)
    }
}

/// What exists only while a seat is signed in and joined.
pub struct SyncRuntime {
    pub backend: Arc<dyn Backend>,
    pub replica: Arc<ReplicaStore>,
    /// One write at a time from this seat. **Never taken by a read**, by navigation, or by the sync task.
    gate: tokio::sync::Mutex<()>,
    /// Bumped whenever the seat's view changes. A write whose view effects arrive after the person has looked
    /// somewhere else drops them ([P2]).
    epoch: AtomicU64,
    pub(crate) worker: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    pub poll: Duration,
}

impl SyncRuntime {
    pub fn new(backend: Arc<dyn Backend>, replica: Arc<ReplicaStore>) -> SyncRuntime {
        SyncRuntime { backend, replica, gate: tokio::sync::Mutex::new(()), epoch: AtomicU64::new(0), worker: Default::default(), poll: POLL_EVERY }
    }
}

fn write_id(ctx: &Ctx) -> String {
    format!("w-{}", model::Ulid::from_parts(ctx.now.at, ctx.entropy))
}

fn offline_words() -> &'static str {
    "not saved — offline: your changes cannot reach the team server right now. Reads still work; nothing was queued. Try again when it is back."
}

/// When, in words a person says: "just now", "2 min ago", "3 h ago", "4 d ago".
fn ago(at_ms: i64, now_ms: i64) -> String {
    let secs = ((now_ms - at_ms) / 1000).max(0);
    match secs {
        0..=44 => "just now".into(),
        45..=3599 => format!("{} min ago", (secs / 60).max(1)),
        3600..=86_399 => format!("{} h ago", secs / 3600),
        _ => format!("{} d ago", secs / 86_400),
    }
}

impl Core {
    pub(crate) fn sync_rt(&self) -> Option<Arc<SyncRuntime>> {
        self.sync.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub(crate) fn push_update(&self, v: Value) {
        if let Some(p) = self.pusher.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            p(v);
        }
    }

    /// Run this seat on the org's replica. Called when an account signs in (and at launch, if a session is
    /// there). The local file is left exactly as it is.
    pub(crate) async fn start_sync(self: &Arc<Self>, backend: Arc<dyn Backend>, replica: Arc<ReplicaStore>, org: model::Org, poll: Duration) -> Result<(), String> {
        if self.sync_rt().is_some() {
            return Err("already syncing".into());
        }
        let (now, _) = clock();
        // The local dataset is on disk before the seat stops running on it.
        {
            let db = self.state.lock().await.db();
            if !self.saves.write_through(db, FLUSH_WAIT).await {
                return Err("could not save the local file before joining; you are not joined".into());
            }
        }
        let _ = replica.put_meta("org", &serde_json::to_string(&org).unwrap_or_default());
        let mut db = replica.load().map_err(|e| format!("cannot read the replica: {e:#}"))?;
        db.org = org;
        let (members, agents) = replica.team();
        let mut rt = SyncRuntime::new(backend, replica.clone());
        rt.poll = poll;
        let rt = Arc::new(rt);
        {
            let mut st = self.state.lock().await;
            st.replace_db(db);
            st.set_sync(SyncInfo { state: SyncState::Syncing, last_seen: None, members, team_agents: agents });
        }
        self.switch.use_store(replica);
        *self.sync.lock().unwrap_or_else(|e| e.into_inner()) = Some(rt.clone());
        let core = self.clone();
        let w = rt.clone();
        let handle = tokio::spawn(async move { core.run_worker(w).await });
        *rt.worker.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle);
        let update = self.state.lock().await.update(now, &[]);
        self.push_update(update);
        Ok(())
    }

    /// Back to the local file, exactly as it was.
    pub(crate) async fn stop_sync(&self) -> Result<(), String> {
        let rt = self.sync.lock().unwrap_or_else(|e| e.into_inner()).take();
        let Some(rt) = rt else { return Ok(()) };
        if let Some(h) = rt.worker.lock().unwrap_or_else(|e| e.into_inner()).take() {
            h.abort();
        }
        // What the seat was looking at and its marks go to the replica it is leaving.
        let db = self.state.lock().await.db();
        let _ = rt.replica.save(&db);
        self.switch.use_local();
        let local = self.switch.load().map_err(|e| format!("cannot read the local file: {e:#}"))?;
        let (now, _) = clock();
        let mut st = self.state.lock().await;
        st.replace_db(local);
        st.set_sync(SyncInfo::default());
        let update = st.update(now, &[]);
        drop(st);
        self.push_update(update);
        Ok(())
    }

    /// Join the org the session belongs to: open its replica and start syncing. (Step 5 puts the person's choice —
    /// bring my local records, or start from the org's — in front of this; until then a seat starts from the org's.)
    pub(crate) async fn join_session(self: &Arc<Self>) -> Result<(), String> {
        let Some((_, org)) = self.account.identity() else { return Ok(()) };
        let dir = clappkit::paths::data_file(CLI, "replica").parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let replica = ReplicaStore::open(&crate::replica::path_for(&dir, &org.id)).map_err(|e| format!("cannot open the org replica: {e:#}"))?;
        let backend = Arc::new(crate::sync::HttpBackend::new(self.account.clone()));
        self.start_sync(backend, Arc::new(replica), org, POLL_EVERY).await
    }

    // MARK: - Catching up

    fn set_link(&self, st: &mut AppState, state: SyncState) {
        let (now, _) = clock();
        st.set_sync_state(state, if state == SyncState::Offline { None } else { Some(now.at) });
    }

    async fn mark_offline(&self) {
        let (now, _) = clock();
        let mut st = self.state.lock().await;
        if st.sync_info().state != SyncState::Offline {
            st.set_sync_state(SyncState::Offline, None);
            let update = st.update(now, &[]);
            drop(st);
            self.push_update(update);
        }
    }

    /// Bring the replica to the org's latest: bootstrap if it holds nothing (or the log cannot reach it), then
    /// apply `changes` until caught up. `Err` means the server could not be reached.
    pub(crate) async fn sync_pass(&self, rt: &Arc<SyncRuntime>) -> Result<(), Offline> {
        let (now, _) = clock();
        {
            let mut st = self.state.lock().await;
            if st.sync_info().state == SyncState::Offline {
                st.set_sync_state(SyncState::Syncing, None);
                let update = st.update(now, &[]);
                drop(st);
                self.push_update(update);
            }
        }
        let mut guard = 0;
        loop {
            guard += 1;
            if guard > 1000 {
                return Ok(());
            }
            if !rt.replica.is_loaded() {
                self.rebootstrap(rt).await?;
                continue;
            }
            let since = self.state.lock().await.watermark();
            let be = rt.backend.clone();
            let changes = tokio::task::spawn_blocking(move || be.changes(since)).await.map_err(|_| Offline("the sync call did not finish".into()))??;
            match changes {
                Changes::Stale { .. } => {
                    let _ = rt.replica.put_meta("loaded", "0");
                    continue;
                }
                Changes::Ops { watermark, to, ops } => {
                    if !ops.is_empty() && self.apply_batch(rt, &ops).await.is_err() {
                        // A hole in the log (or a row that would not save): start from the org's truth.
                        let _ = rt.replica.put_meta("loaded", "0");
                        continue;
                    }
                    if ops.is_empty() || to >= watermark {
                        let mut st = self.state.lock().await;
                        let was = st.sync_info().state;
                        self.set_link(&mut st, SyncState::Online);
                        if was != SyncState::Online {
                            let update = st.update(now, &[]);
                            drop(st);
                            self.push_update(update);
                        }
                        return Ok(());
                    }
                }
            }
        }
    }

    async fn rebootstrap(&self, rt: &Arc<SyncRuntime>) -> Result<(), Offline> {
        let be = rt.backend.clone();
        let b = tokio::task::spawn_blocking(move || be.bootstrap()).await.map_err(|_| Offline("the bootstrap did not finish".into()))??;
        rt.replica.replace_all(&b).map_err(|e| Offline(format!("cannot save the replica: {e:#}")))?;
        let (now, _) = clock();
        let mut st = self.state.lock().await;
        st.load_bootstrap(&b).map_err(Offline)?;
        let update = st.update(now, &[]);
        drop(st);
        self.push_update(update);
        Ok(())
    }

    /// Apply a batch of the org's log, persist it, publish one `update`. `Err` = a hole or an unsaveable batch;
    /// nothing was applied.
    async fn apply_batch(&self, rt: &Arc<SyncRuntime>, ops: &[crate::sync::ServerOp]) -> Result<Vec<Op>, ()> {
        let (now, _) = clock();
        let mut st = self.state.lock().await;
        let w0 = st.watermark();
        let cp = st.checkpoint();
        let delta = match st.apply_server_ops(ops) {
            Ok(d) => d,
            Err(_) => {
                st.roll_back_command(cp);
                return Err(());
            }
        };
        let w1 = st.watermark();
        let applied: Vec<_> = ops.iter().filter(|o| o.seq > w0 && o.seq <= w1).cloned().collect();
        if !applied.is_empty() {
            if rt.replica.apply(&applied, w1).is_err() {
                st.roll_back_command(cp);
                return Err(());
            }
            st.set_sync_state(SyncState::Online, Some(now.at));
            let mut update = st.update(now, &delta);
            drop(st);
            if let Some(d) = update.get_mut("delta") {
                d["from"] = json!(w0);
            }
            self.push_update(update);
            let bys: Vec<Value> = delta.iter().map(|o| json!(o.by)).collect();
            self.meet(rt, &bys).await;
        }
        Ok(delta)
    }

    /// If any of these actors is someone the seat has not met, ask the org who is in it now. Best effort.
    async fn meet(&self, rt: &Arc<SyncRuntime>, actors: &[Value]) {
        let unknown = {
            let st = self.state.lock().await;
            actors.iter().any(|a| !a.is_null() && !st.knows_actor(a))
        };
        if !unknown {
            return;
        }
        let be = rt.backend.clone();
        if let Ok(Ok((members, agents))) = tokio::task::spawn_blocking(move || be.team()).await {
            let _ = rt.replica.put_team(&members, &agents);
            let (now, _) = clock();
            let mut st = self.state.lock().await;
            st.set_team(members, agents);
            let update = st.update(now, &[]);
            drop(st);
            self.push_update(update);
        }
    }

    /// The seat's link to the org, for as long as it is signed in: catch up, listen, and when the channel is
    /// down, ask the log every [`POLL_EVERY`] so a teammate's change still arrives within the bound.
    pub(crate) async fn run_worker(self: Arc<Self>, rt: Arc<SyncRuntime>) {
        loop {
            if self.sync_pass(&rt).await.is_err() {
                self.mark_offline().await;
                tokio::time::sleep(rt.poll).await;
                continue;
            }
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<u64>();
            let be = rt.backend.clone();
            let mut listening = tokio::task::spawn_blocking(move || be.stream(&mut |w| tx.send(w).is_ok()));
            loop {
                tokio::select! {
                    mark = rx.recv() => match mark {
                        Some(w) => {
                            let mine = self.state.lock().await.watermark();
                            if w > mine && self.sync_pass(&rt).await.is_err() {
                                self.mark_offline().await;
                                break;
                            }
                        }
                        None => break,
                    },
                    _ = &mut listening => break,
                }
            }
            // The channel is down: poll, and try the channel again, every `poll`.
            tokio::time::sleep(rt.poll).await;
        }
    }

    // MARK: - Commands while signed in

    /// `Core::command` for a signed-in seat. Reads and navigation finish as in local mode, under the lock, and
    /// never wait on the network. A command that changes rows is **sent**: the lock is released, the write gate
    /// taken, the command re-run from the then-current state, and the batch confirmed by the server before anything
    /// is acknowledged.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn command_synced(
        &self,
        rt: Arc<SyncRuntime>,
        req: Value,
        caller: Option<String>,
        roster: Vec<clappkit::AgentRow>,
        workspace_agents: std::collections::HashMap<String, String>,
        now: Now,
        ctx: Ctx,
    ) -> Reply {
        // First run: is this a write at all?
        {
            let mut st = self.state.lock().await;
            let cp = st.checkpoint();
            let view0 = st.view_snapshot();
            st.reset_acting();
            st.set_agents(roster.clone());
            st.set_workspace_agents(workspace_agents.clone());
            let out = st.command(&req, caller.as_deref(), &ctx);
            if row_ops(cp.db(), st.db_ref()).is_empty() {
                return self.finish_without_rows(&rt, st, cp, view0, out, caller, now).await;
            }
            st.roll_back_command(cp);
        }

        // A write. One at a time from this seat; reads and navigation do not queue behind it.
        let _gate = rt.gate.lock().await;
        let (out, ops, after_view, epoch, by, feed_before) = {
            let mut st = self.state.lock().await;
            let cp = st.checkpoint();
            st.reset_acting();
            st.set_agents(roster);
            st.set_workspace_agents(workspace_agents);
            let feed_before = st.feed_seq();
            let out = st.command(&req, caller.as_deref(), &ctx);
            let ops = row_ops(cp.db(), st.db_ref());
            let by = st.acting();
            let after_view = st.view_snapshot();
            st.roll_back_db(cp); // the rows change only when the server has said yes; the feed line stays
            (out, ops, after_view, rt.epoch.load(Ordering::SeqCst), by, feed_before)
        };
        if ops.is_empty() {
            // The state moved under us and the command turned out to be a no-op: it is a read.
            let resp = out.resp;
            let push = resp["split"].clone();
            return Reply::new(resp, push);
        }

        let (write_id, be, sent) = (write_id(&ctx), rt.backend.clone(), ops.clone());
        let by2 = by.clone();
        let wid = write_id.clone();
        let answer = tokio::task::spawn_blocking(move || {
            // A dropped connection is tried once more with the same write id: the server cannot apply it twice.
            match be.write(&wid, &by2, &sent) {
                Err(_) => be.write(&wid, &by2, &sent),
                ok => ok,
            }
        })
        .await
        .unwrap_or_else(|_| Err(Offline("the write did not finish".into())));

        match answer {
            Err(Offline(_)) => {
                self.mark_offline().await;
                self.refuse(feed_before, offline_words(), json!({ "reason": "offline" })).await
            }
            Ok(WriteAnswer::Refused { code, reason }) => {
                let msg = format!("not saved — {reason}");
                self.refuse(feed_before, &msg, json!({ "reason": "rule", "code": code })).await
            }
            Ok(WriteAnswer::Stale { kind, id, current, by: who, at }) => {
                // Show the truth first: the row as the org has it now goes into the replica.
                let _ = self.take_current(&rt, &kind, &current).await;
                let (handle, saying) = self.describe_current(&rt, &kind, &id, &current, &who, at).await;
                let msg = format!("not saved — {handle} changed while you were looking: {saying}. Nothing was changed; look again and retry.");
                let refused = json!({ "reason": "stale", "kind": kind, "id": id, "handle": handle, "by": who, "at": at, "current": current });
                self.refuse(feed_before, &msg, refused).await
            }
            Ok(WriteAnswer::Accepted { seq_to, rows, .. }) => {
                // Bring the replica to `seq_to` along the log, then answer.
                let mut delta: Vec<Op> = Vec::new();
                let mut guard = 0;
                loop {
                    guard += 1;
                    let (have, be) = (self.state.lock().await.watermark(), rt.backend.clone());
                    if have >= seq_to || guard > 50 {
                        break;
                    }
                    let got = tokio::task::spawn_blocking(move || be.changes(have)).await;
                    match got {
                        Ok(Ok(Changes::Ops { ops, .. })) if !ops.is_empty() => match self.apply_batch(&rt, &ops).await {
                            Ok(d) => delta.extend(d),
                            Err(()) => break,
                        },
                        _ => break, // offline now, or stale: the worker catches up; the write is already the server's
                    }
                }
                let mut resp = out.resp;
                {
                    let mut st = self.state.lock().await;
                    if rt.epoch.load(Ordering::SeqCst) == epoch {
                        st.adopt_view(after_view);
                        rt.epoch.fetch_add(1, Ordering::SeqCst);
                    }
                    // What the records were finally called.
                    let finals: std::collections::HashMap<&str, &str> =
                        rows.iter().filter_map(|r| Some((r.id.as_str(), r.handle.as_deref()?))).collect();
                    let created: Vec<(&'static str, String, String)> = ops
                        .iter()
                        .filter(|o| o.op == "create" && crate::sync::ROW_KINDS.contains(&o.kind))
                        .filter_map(|o| Some((o.kind, o.record.get("handle")?.as_str()?.to_string(), finals.get(o.id.as_str())?.to_string())))
                        .collect();
                    resp["handles"] = json!(created.iter().map(|(k, p, f)| json!({ "kind": k, "proposed": p, "final": f })).collect::<Vec<_>>());
                    let mut update = st.update(now, &delta);
                    if let Some(first) = delta.first() {
                        update["delta"]["from"] = json!(first.seq.saturating_sub(1));
                    }
                    resp["split"] = update.clone();
                    drop(st);
                    if caller.is_none() {
                        self.control.emit_all(out.emits);
                    }
                    let push = resp["split"].clone();
                    Reply::new(resp, push)
                }
            }
        }
    }

    /// A command that produced no row ops: it is the command. Persist what it changed (the view; the seat's own
    /// marks) the way local mode does — except that nothing is numbered, because the watermark is the org's.
    #[allow(clippy::too_many_arguments)]
    async fn finish_without_rows(
        &self,
        rt: &Arc<SyncRuntime>,
        mut st: tokio::sync::MutexGuard<'_, AppState>,
        cp: crate::state::replicate::Checkpoint,
        view0: state::View,
        out: state::Outcome,
        caller: Option<String>,
        now: Now,
    ) -> Reply {
        if st.view_snapshot() != view0 {
            rt.epoch.fetch_add(1, Ordering::SeqCst);
        }
        match st.db_ref().change_from(cp.db()) {
            model::Change::None => {}
            model::Change::View => self.saves.save(st.db()),
            model::Change::Data => {
                if !self.saves.write_through(st.db(), FLUSH_WAIT).await {
                    st.roll_back_command(cp);
                    let update = st.update(now, &[]);
                    let resp = json!({ "ok": false, "error": NOT_SAVED, "split": update });
                    return Reply::new(resp, update);
                }
            }
        }
        if caller.is_none() {
            self.control.emit_all(out.emits);
        }
        Reply::new(out.resp.clone(), out.resp["split"].clone())
    }

    async fn refuse(&self, feed_before: u64, message: &str, refused: Value) -> Reply {
        let (now, _) = clock();
        let mut st = self.state.lock().await;
        st.refuse_last_call(feed_before, message);
        let update = st.update(now, &[]);
        drop(st);
        let resp = json!({ "ok": false, "error": message, "refused": refused, "split": update });
        Reply::new(resp, update)
    }

    /// Put the row the server says is current into the replica, as the log would have.
    async fn take_current(&self, rt: &Arc<SyncRuntime>, kind: &str, current: &Value) -> Result<(), ()> {
        let Some(version) = current.get("version").and_then(Value::as_u64) else { return Err(()) };
        let id = current.get("id").and_then(Value::as_str).unwrap_or("").to_string();
        let (now, _) = clock();
        let mut st = self.state.lock().await;
        let held = st.db_ref().versions.get(&id).copied().unwrap_or(0);
        if version <= held {
            return Ok(());
        }
        // Not a log entry (the watermark does not move): just the row, so the next draw is the truth.
        let cp = st.checkpoint();
        if !st.put_current_row(kind, current) {
            st.roll_back_command(cp);
            return Err(());
        }
        let _ = rt.replica.apply(
            &[crate::sync::ServerOp { seq: version, op: "upsert".into(), kind: kind.to_string(), id, record: current.clone(), by: Value::Null, at: 0 }],
            st.watermark(),
        );
        let update = st.update(now, &[]);
        drop(st);
        self.push_update(update);
        Ok(())
    }

    /// `(handle, "its value is now … (set by Ana, 2 min ago)")` for a stale refusal.
    async fn describe_current(&self, rt: &Arc<SyncRuntime>, kind: &str, id: &str, current: &Value, who: &Value, at: i64) -> (String, String) {
        self.meet(rt, std::slice::from_ref(who)).await;
        let (now, _) = clock();
        let st = self.state.lock().await;
        let handle = current.get("handle").and_then(Value::as_str).unwrap_or(id).to_string();
        let by = st.name_of_actor(who);
        let said = st.describe_row_now(kind, current);
        (handle, format!("{said} (set by {by}, {})", ago(at, now.at)))
    }
}
