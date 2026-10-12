//! **The org replica** (p3, `docs/sync-contract.md` §5.2): SQLite behind [`CrmStore`], written **per row**.
//!
//! A signed-in seat runs on this instead of `crm.json`. Applying a batch of the org's log — rows, feed items, each
//! row's version and the new watermark — is **one transaction**, so the watermark reaches the disk with the rows it
//! numbers (the p1 invariant) without rewriting the dataset. `save` writes only what is *not* a server row: the
//! seat's view, its reminders and its reminder marks, so a navigation never touches a row table.
//!
//! `crm.json` is a different file and is never opened here.

use crate::model::*;
use crate::store::CrmStore;
use crate::sync::{put_feed, put_row, Bootstrap, ServerOp, ROW_KINDS};
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS rows (kind TEXT NOT NULL, id TEXT NOT NULL, version INTEGER NOT NULL, json TEXT NOT NULL, PRIMARY KEY (kind, id));
CREATE TABLE IF NOT EXISTS feed (kind TEXT NOT NULL, id TEXT NOT NULL, seq INTEGER NOT NULL, json TEXT NOT NULL, PRIMARY KEY (kind, id));
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS seat_marks (task_id TEXT PRIMARY KEY, due_sent_at INTEGER NOT NULL);
";

pub struct ReplicaStore {
    conn: Mutex<Connection>,
}

/// `replica-<orgId>.sqlite` in the data dir. The org id is the server's (opaque, `org_…`), but it still goes
/// through a filter: a file name is not the place to trust a string.
pub fn path_for(dir: &Path, org_id: &str) -> PathBuf {
    let safe: String = org_id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
    dir.join(format!("replica-{safe}.sqlite"))
}

impl ReplicaStore {
    /// Open (or create) the replica at `path`. A file that is not a database is moved aside and replaced: a
    /// replica is the server's data, re-downloadable, so refusing to start would cost more than starting fresh.
    pub fn open(path: &Path) -> Result<ReplicaStore> {
        let first = Connection::open(path).and_then(|c| {
            c.pragma_update(None, "journal_mode", "WAL")?;
            c.pragma_update(None, "synchronous", "FULL")?;
            c.execute_batch(SCHEMA)?;
            Ok(c)
        });
        let conn = match first {
            Ok(c) => c,
            Err(_) => {
                let _ = clappkit::store::quarantine(path);
                for ext in ["-wal", "-shm"] {
                    let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
                }
                let c = Connection::open(path).with_context(|| format!("cannot open {}", path.display()))?;
                c.pragma_update(None, "journal_mode", "WAL")?;
                c.pragma_update(None, "synchronous", "FULL")?;
                c.execute_batch(SCHEMA)?;
                c
            }
        };
        Ok(ReplicaStore { conn: Mutex::new(conn) })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn meta(&self, key: &str) -> Option<String> {
        self.conn().query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).optional().ok().flatten()
    }

    pub fn put_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn().execute("INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![key, value])?;
        Ok(())
    }

    /// Does this replica hold a bootstrap (as opposed to being a freshly created file)?
    pub fn is_loaded(&self) -> bool {
        self.meta("loaded").as_deref() == Some("1")
    }

    /// Replace everything the org holds with a bootstrap, in one transaction.
    pub fn replace_all(&self, b: &Bootstrap) -> Result<()> {
        let mut c = self.conn();
        let tx = c.transaction()?;
        tx.execute_batch("DELETE FROM rows; DELETE FROM feed;")?;
        for (kind, row) in &b.rows {
            insert(&tx, kind, row, 0)?;
        }
        meta(&tx, "watermark", &b.watermark.to_string())?;
        meta(&tx, "pipeline", &b.pipeline.to_string())?;
        meta(&tx, "members", &serde_json::to_string(&b.members)?)?;
        meta(&tx, "agents", &serde_json::to_string(&b.agents)?)?;
        meta(&tx, "loaded", "1")?;
        tx.commit()?;
        Ok(())
    }

    /// Members and agents, as last bootstrapped.
    pub fn team(&self) -> crate::sync::Team {
        let get = |k: &str| self.meta(k).unwrap_or_default();
        (serde_json::from_str(&get("members")).unwrap_or_default(), serde_json::from_str(&get("agents")).unwrap_or_default())
    }

    pub fn put_team(&self, members: &[(String, String)], agents: &[(String, String, String)]) -> Result<()> {
        self.put_meta("members", &serde_json::to_string(members)?)?;
        self.put_meta("agents", &serde_json::to_string(agents)?)
    }
}

fn meta(tx: &rusqlite::Transaction<'_>, key: &str, value: &str) -> Result<()> {
    tx.execute("INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![key, value])?;
    Ok(())
}

fn insert(tx: &rusqlite::Transaction<'_>, kind: &str, row: &Value, seq: u64) -> Result<()> {
    let id = row.get("id").and_then(Value::as_str).context("a row has an id")?;
    if ROW_KINDS.contains(&kind) {
        let version = row.get("version").and_then(Value::as_u64).unwrap_or(seq);
        tx.execute(
            "INSERT INTO rows (kind, id, version, json) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(kind, id) DO UPDATE SET version = excluded.version, json = excluded.json WHERE excluded.version > rows.version",
            params![kind, id, version as i64, row.to_string()],
        )?;
    } else {
        tx.execute("INSERT OR IGNORE INTO feed (kind, id, seq, json) VALUES (?1, ?2, ?3, ?4)", params![kind, id, seq as i64, row.to_string()])?;
    }
    Ok(())
}

impl CrmStore for ReplicaStore {
    /// The dataset the core runs on: every row and feed item, versions, the watermark, the seat's view, reminders
    /// and marks.
    fn load(&self) -> Result<Db> {
        let c = self.conn();
        let mut db = Db::default();
        if let Some(org) = c.query_row("SELECT value FROM meta WHERE key = 'org'", [], |r| r.get::<_, String>(0)).optional()? {
            db.org = serde_json::from_str(&org).unwrap_or_default();
        }
        let get = |k: &str| c.query_row("SELECT value FROM meta WHERE key = ?1", [k], |r| r.get::<_, String>(0)).optional().ok().flatten();
        let pipeline: Option<Value> = get("pipeline").and_then(|s| serde_json::from_str(&s).ok());
        let mut stmt = c.prepare("SELECT kind, json FROM rows ORDER BY kind, id")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (kind, json) = row?;
            if let Ok(v) = serde_json::from_str::<Value>(&json) {
                let _ = put_row(&mut db, &kind, &v);
            }
        }
        let mut stmt = c.prepare("SELECT kind, json FROM feed ORDER BY seq, id")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (kind, json) = row?;
            if let Ok(v) = serde_json::from_str::<Value>(&json) {
                let _ = put_feed(&mut db, &kind, &v);
            }
        }
        // Stable creation order, the way the local file keeps it (ids are time-ordered).
        db.companies.sort_by(|a, b| a.id.cmp(&b.id));
        db.contacts.sort_by(|a, b| a.id.cmp(&b.id));
        db.deals.sort_by(|a, b| a.id.cmp(&b.id));
        db.tasks.sort_by(|a, b| a.id.cmp(&b.id));
        db.activities.sort_by(|a, b| a.id.cmp(&b.id));
        db.moves.sort_by(|a, b| a.id.cmp(&b.id));
        if let Some(p) = pipeline {
            if let Some(id) = p.get("id").and_then(Value::as_str) {
                db.pipelines = vec![Pipeline {
                    id: id.to_string(),
                    name: p.get("name").and_then(Value::as_str).unwrap_or("Sales").to_string(),
                    stages: p.get("stages").and_then(Value::as_array).map(|a| a.iter().filter_map(|s| Stage::parse(s.as_str()?)).collect()).unwrap_or_else(|| Stage::ALL.to_vec()),
                }];
            }
        }
        db.watermark = get("watermark").and_then(|s| s.parse().ok()).unwrap_or(0);
        if let Some(v) = get("view").and_then(|s| serde_json::from_str(&s).ok()) {
            db.view = v;
        }
        if let Some(r) = get("reminders").and_then(|s| serde_json::from_str(&s).ok()) {
            db.reminders = r;
        }
        let mut stmt = c.prepare("SELECT task_id, due_sent_at FROM seat_marks")?;
        let marks: std::collections::HashMap<String, i64> =
            stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?.filter_map(|r| r.ok()).collect();
        for t in &mut db.tasks {
            t.due_sent_at = marks.get(&t.id).copied();
        }
        Ok(db)
    }

    /// Only what is not a server row: the view, the reminders, the marks, and the org's name.
    fn save(&self, db: &Db) -> Result<()> {
        let mut c = self.conn();
        let tx = c.transaction()?;
        meta(&tx, "view", &serde_json::to_string(&db.view)?)?;
        meta(&tx, "reminders", &serde_json::to_string(&db.reminders)?)?;
        meta(&tx, "org", &serde_json::to_string(&db.org)?)?;
        tx.execute("DELETE FROM seat_marks", [])?;
        for t in &db.tasks {
            if let Some(m) = t.due_sent_at {
                tx.execute("INSERT INTO seat_marks (task_id, due_sent_at) VALUES (?1, ?2)", params![t.id, m])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// One batch of the org's log, in one transaction with the watermark it reaches.
    fn apply(&self, ops: &[ServerOp], watermark: u64) -> Result<()> {
        let mut c = self.conn();
        let tx = c.transaction()?;
        for op in ops {
            insert(&tx, &op.kind, &op.record, op.seq)?;
        }
        meta(&tx, "watermark", &watermark.to_string())?;
        tx.commit()?;
        Ok(())
    }
}
