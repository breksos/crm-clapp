//! **p3 step 5: joining an org that already has data** — `docs/sync-contract.md` §7.
//!
//! The choice is the person's, in the window, and never automatic. Until they answer, the seat is signed in but not
//! joined: `sync.state` is `choosing`, record writes are refused, reads work, and **nothing leaves the machine**.
//!
//! * **`start-fresh`** — the replica is the org's. Nothing is uploaded.
//! * **`bring`** — the local file's records go up through the same write path as everything else, in dependency order,
//!   in atomic parts; the server resolves handle collisions and the final handles are reported.
//! * Either way `crm.json` is left exactly as it is.
//! * A second account on a machine whose file was claimed by the first is offered `start-fresh` only, with the reason.

use super::*;
use crate::replica::ReplicaStore;
use crate::state::replicate::SyncState;
use crate::sync::{join_done, join_plan, join_refused, row_ops, Backend, Bootstrap, Counts, HandleChange, Offline, WriteAnswer, CLAIMED_BY_ANOTHER, ROW_KINDS};
use model::{Actor, Db, Org, Person};
use std::collections::{HashMap, HashSet};

/// Operations per atomic part of a `bring`. The server takes 10 000; a part this size is a few hundred KB.
pub const BRING_PART: usize = 1000;

/// What a signed-in seat is holding while it waits for the person to choose.
pub struct PendingJoin {
    backend: Arc<dyn Backend>,
    replica: Arc<ReplicaStore>,
    org: Org,
    boot: Bootstrap,
    poll: Duration,
    /// Set when the local file's records belong to somebody else: `bring` is not offered.
    reason: Option<&'static str>,
}

fn org_counts(b: &Bootstrap) -> Counts {
    let mut c = Counts::default();
    for (kind, _) in &b.rows {
        match kind.as_str() {
            "company" => c.companies += 1,
            "contact" => c.contacts += 1,
            "deal" => c.deals += 1,
            "task" => c.tasks += 1,
            "activity" => c.activities += 1,
            "move" => c.moves += 1,
            _ => {}
        }
    }
    c.total = c.companies + c.contacts + c.deals + c.tasks + c.activities + c.moves;
    c
}

/// Does anything in the local file belong to someone who is neither the person signing in nor the local person?
/// (The first account to sign in on an install claimed the file, p2; a second one must not take it.)
fn belongs_to_another(db: &Db, me: &Person, local: &Person) -> bool {
    let foreign = |owner: &str| !owner.is_empty() && owner != me.id && owner != local.id;
    db.companies.iter().any(|r| foreign(&r.owner_id))
        || db.contacts.iter().any(|r| foreign(&r.owner_id))
        || db.deals.iter().any(|r| foreign(&r.owner_id))
        || db.tasks.iter().any(|r| foreign(&r.owner_id))
        || db.activities.iter().any(|r| foreign(&r.owner_id))
}

/// The agent ids the local records name, in the order they first appear.
fn agents_named(db: &Db) -> Vec<String> {
    let mut seen = Vec::<String>::new();
    let mut note = |a: &Actor| {
        if let Actor::Agent { id } = a {
            if !seen.contains(id) {
                seen.push(id.clone());
            }
        }
    };
    db.deals.iter().for_each(|r| note(&r.moved_by));
    db.tasks.iter().for_each(|r| note(&r.by));
    db.activities.iter().for_each(|r| note(&r.by));
    db.moves.iter().for_each(|r| note(&r.by));
    seen
}

/// Who a local record says did something, as the org can hear it. A person, whatever id the local file knew them
/// by, is the signing-in person. **An agent is always an agent**: its work is never credited to the person. `ids`
/// maps each local agent id to the workspace id the server holds for it (registered before anything is sent).
fn claim_actor(a: &Actor, me: &Person, local: &Person, ids: &HashMap<String, String>) -> Actor {
    match a {
        Actor::Human { id } if *id == me.id || *id == local.id || id.is_empty() => Actor::Human { id: me.id.clone() },
        Actor::Human { .. } => Actor::Human { id: me.id.clone() },
        Actor::Agent { id } => Actor::Agent { id: ids.get(id).cloned().unwrap_or_else(|| id.clone()) },
    }
}

fn normalised(db: &Db, me: &Person, local: &Person, ids: &HashMap<String, String>) -> Db {
    let mut d = db.clone();
    let fix = |a: &mut Actor| *a = claim_actor(a, me, local, ids);
    d.deals.iter_mut().for_each(|r| fix(&mut r.moved_by));
    d.tasks.iter_mut().for_each(|r| fix(&mut r.by));
    d.activities.iter_mut().for_each(|r| fix(&mut r.by));
    d.moves.iter_mut().for_each(|r| fix(&mut r.by));
    d
}

/// What an agent the local file never gave a name is called in the org.
pub const UNNAMED_AGENT: &str = "an agent (unnamed)";

fn tally(c: &mut Counts, kind: &str) {
    match kind {
        "company" => c.companies += 1,
        "contact" => c.contacts += 1,
        "deal" => c.deals += 1,
        "task" => c.tasks += 1,
        "activity" => c.activities += 1,
        "move" => c.moves += 1,
        _ => return,
    }
    c.total += 1;
}

impl Core {
    pub(crate) fn pending_join(&self) -> Option<Arc<PendingJoin>> {
        self.joining.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Sign-in found an org to join. If the seat has nothing local, or already joined this org before, it just
    /// joins. Otherwise it waits for the person: nothing is sent, and the window is told to ask.
    pub(crate) async fn begin_join(self: &Arc<Self>, backend: Arc<dyn Backend>, replica: Arc<ReplicaStore>, org: Org, poll: Duration) -> Result<(), String> {
        if replica.meta("joinChoice").is_some() || replica.is_loaded() {
            return self.start_sync(backend, replica, org, poll).await;
        }
        let be = backend.clone();
        let boot = tokio::task::spawn_blocking(move || be.bootstrap())
            .await
            .map_err(|_| "the join did not finish".to_string())?
            .map_err(|Offline(m)| format!("cannot reach the server to join ({m})"))?;
        let (db, me) = {
            let st = self.state.lock().await;
            (st.db(), st.me().clone())
        };
        let local = Counts::of(&db);
        if local.total == 0 {
            // Nothing to bring and so nothing to ask.
            replica.replace_all(&boot).map_err(|e| format!("cannot save the replica: {e:#}"))?;
            let _ = replica.put_meta("joinChoice", "none");
            return self.start_sync(backend, replica, org, poll).await;
        }
        let reason = belongs_to_another(&db, &me, &self.local_person).then_some(CLAIMED_BY_ANOTHER);
        *self.joining.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(PendingJoin { backend, replica, org, boot, poll, reason }));
        let (now, _) = clock();
        let mut st = self.state.lock().await;
        st.set_sync_state(SyncState::Choosing, None);
        let update = st.update(now, &[]);
        drop(st);
        self.push_update(update);
        Ok(())
    }

    /// `sync_join_plan()`.
    pub(crate) async fn join_plan(&self) -> Value {
        match self.pending_join() {
            Some(p) => {
                let local = Counts::of(&self.state.lock().await.db());
                join_plan(local, org_counts(&p.boot), p.reason)
            }
            None => join_plan(Counts::default(), Counts::default(), None),
        }
    }

    /// `sync_join(choice)`.
    pub(crate) async fn join(self: &Arc<Self>, choice: &str, part_size: usize) -> Value {
        let Some(p) = self.pending_join() else {
            return json!({ "ok": false, "error": "there is nothing to join right now", "choice": choice });
        };
        match choice {
            "start-fresh" => {
                if let Err(e) = p.replica.replace_all(&p.boot) {
                    return json!({ "ok": false, "choice": choice, "state": "choosing", "error": format!("cannot save the replica: {e:#}") });
                }
                self.finish_join(&p, "start-fresh").await.map(|_| join_done("start-fresh", Counts::default(), 0, &[])).unwrap_or_else(|e| json!({ "ok": false, "choice": choice, "state": "choosing", "error": e }))
            }
            "bring" => self.bring(&p, part_size.max(1)).await,
            other => json!({ "ok": false, "choice": other, "state": "choosing", "error": "choose \"bring\" or \"start-fresh\"" }),
        }
    }

    async fn finish_join(self: &Arc<Self>, p: &Arc<PendingJoin>, choice: &str) -> Result<(), String> {
        let _ = p.replica.put_meta("joinChoice", choice);
        *self.joining.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.start_sync(p.backend.clone(), p.replica.clone(), p.org.clone(), p.poll).await
    }

    async fn bring(self: &Arc<Self>, p: &Arc<PendingJoin>, part_size: usize) -> Value {
        let zero = Counts::default();
        let refuse = |msg: &str, up: Counts, done: usize, total: usize, changes: &[HandleChange]| join_refused(msg, up, done, total, None, changes);
        if p.reason.is_some() {
            return refuse("this file's records belong to another account — start from the org's instead", zero, 0, 0, &[]);
        }
        let (db, me) = {
            let st = self.state.lock().await;
            (st.db(), st.me().clone())
        };
        // Every agent the records name must exist in the org **before** anything is sent, under the name the file
        // recorded for it (or "an agent (unnamed)"): its work keeps its own attribution, and a failure here stops
        // the whole upload with nothing sent.
        let minted: HashSet<String> = p.boot.agents.iter().filter(|(_, _, owner)| *owner == me.id).map(|(id, _, _)| id.clone()).collect();
        let known = self.account.agent_ids();
        let mut ids: HashMap<String, String> = HashMap::new();
        for local in agents_named(&db) {
            if minted.contains(&local) {
                continue; // already a workspace id of this person's
            }
            if let Some(ws) = known.get(&local).filter(|ws| minted.contains(*ws)) {
                ids.insert(local, ws.clone());
                continue;
            }
            let name = {
                let st = self.state.lock().await;
                st.recorded_name(&local).filter(|n| !n.trim().is_empty()).unwrap_or_else(|| UNNAMED_AGENT.to_string())
            };
            let name: String = name.chars().take(80).collect();
            let (be, l2) = (p.backend.clone(), local.clone());
            let registered = tokio::task::spawn_blocking(move || be.register_agent(&l2, &name)).await.unwrap_or_else(|_| Err(Offline("the registration did not finish".into())));
            match registered {
                Ok(ws) => {
                    self.account.remember_agent(&local, &ws);
                    ids.insert(local, ws);
                }
                Err(Offline(m)) => return refuse(&format!("not saved — an agent whose work you are bringing could not be registered with the team ({m}); nothing was sent"), zero, 0, 0, &[]),
            }
        }
        let norm = normalised(&db, &me, &self.local_person, &ids);
        let ops = row_ops(&Db::default(), &norm);
        let parts: Vec<&[crate::sync::RowOp]> = ops.chunks(part_size).collect();
        let by = Actor::Human { id: me.id.clone() };
        let (mut uploaded, mut changes) = (Counts::default(), Vec::<HandleChange>::new());

        for (i, part) in parts.iter().enumerate() {
            // The same id for the same part, so a retry after a lost answer is the server's stored answer, not a duplicate.
            let wid = format!("join-{}-{}-{}", p.org.id, i, part[0].id);
            let (be, by2, sent) = (p.backend.clone(), by.clone(), part.to_vec());
            let answer = tokio::task::spawn_blocking(move || match be.write(&wid, &by2, &sent) {
                Err(_) => be.write(&wid, &by2, &sent),
                ok => ok,
            })
            .await
            .unwrap_or_else(|_| Err(Offline("the upload did not finish".into())));
            match answer {
                Ok(WriteAnswer::Accepted { rows, .. }) => {
                    let finals: HashMap<&str, &str> = rows.iter().filter_map(|r| Some((r.id.as_str(), r.handle.as_deref()?))).collect();
                    for op in part.iter() {
                        tally(&mut uploaded, op.kind);
                        if let (true, Some(proposed), Some(fin)) = (ROW_KINDS.contains(&op.kind), op.record.get("handle").and_then(Value::as_str), finals.get(op.id.as_str())) {
                            if proposed != *fin {
                                changes.push(HandleChange { kind: op.kind.to_string(), proposed: proposed.to_string(), final_handle: fin.to_string() });
                            }
                        }
                    }
                }
                Ok(WriteAnswer::Refused { reason, .. }) => return refuse(&format!("not saved — {reason}"), uploaded, i, parts.len(), &changes),
                Ok(WriteAnswer::Stale { .. }) => return refuse("not saved — the team's data changed while your records were going up; try again", uploaded, i, parts.len(), &changes),
                Err(_) => {
                    return refuse(
                        "not saved — offline: your records cannot reach the team server right now. What already went up stays; try again when it is back.",
                        uploaded,
                        i,
                        parts.len(),
                        &changes,
                    )
                }
            }
        }
        match self.finish_join(p, "bring").await {
            Ok(()) => join_done("bring", uploaded, parts.len(), &changes),
            Err(e) => refuse(&e, uploaded, parts.len(), parts.len(), &changes),
        }
    }

    /// Signed in, not joined: reads and navigation work; anything that would change a record is refused, and
    /// nothing is sent.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn command_choosing(
        &self,
        req: Value,
        caller: Option<String>,
        roster: Vec<clappkit::AgentRow>,
        workspace_agents: HashMap<String, String>,
        now: Now,
        ctx: Ctx,
    ) -> Reply {
        let mut st = self.state.lock().await;
        let cp = st.checkpoint();
        let view0 = st.view_snapshot();
        let feed_before = st.feed_seq();
        st.reset_acting();
        st.set_agents(roster);
        st.set_workspace_agents(workspace_agents);
        let out = st.command(&req, caller.as_deref(), &ctx);
        if row_ops(cp.db(), st.db_ref()).is_empty() {
            return self.finish_without_rows(None, st, cp, view0, out, caller, now).await;
        }
        st.roll_back_db(cp);
        let msg = "not saved — choose how to join the team first (bring your records, or start from the team's). Nothing was changed.";
        st.refuse_last_call(feed_before, msg);
        let update = st.update(now, &[]);
        drop(st);
        let resp = json!({ "ok": false, "error": msg, "refused": { "reason": "choosing" }, "split": update });
        Reply::new(resp, update)
    }
}
