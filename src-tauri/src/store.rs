//! Persistence, behind a port.
//!
//! [`AppState`](crate::state::AppState) never touches a file. It is handed a [`Db`] at
//! startup and hands one back to be written; everything between the core and the disk is
//! here. That is what lets every rule in the core be tested without a window server, a
//! temp directory or a filesystem at all.
//!
//! **Be clear-eyed about what this seam buys: file-format independence, not query
//! pushdown.** A future `SqliteStore` implements the same two methods and nothing above it
//! changes — but it would still be loading the whole set into memory. Real query pushdown
//! is a larger change, and pretending otherwise now would be the expensive kind of
//! optimism.

use crate::model::{Db, InstanceId};
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// The whole of what the core needs from storage. Two methods, no queries, no cursors —
/// anything richer would be a promise the JSON implementation cannot keep, and a seam that
/// lies is worse than no seam.
pub trait CrmStore: Send + Sync {
    fn load(&self) -> Result<Db>;
    fn save(&self, db: &Db) -> Result<()>;
}

/// v1: the whole dataset as one JSON file, written atomically.
///
/// Survivable because activities are append-only — the hot path is pushing onto a list,
/// not rewriting a graph. When the file starts hurting (call it ten thousand records) the
/// swap is one impl.
pub struct JsonStore {
    path: PathBuf,
}

impl JsonStore {
    /// A store at an explicit path. Used by tests, which must never write into the
    /// person's real data directory.
    pub fn at(path: impl Into<PathBuf>) -> JsonStore {
        JsonStore { path: path.into() }
    }

    /// The real one: `~/.clatch/appdata/com.breksos.crm/crm.json`.
    ///
    /// The one directory an app may write, so uninstall can erase the whole footprint.
    /// clappkit resolves it from `CLATCH_DATA_DIR` when Clatch injected one.
    pub fn in_data_dir() -> JsonStore {
        JsonStore::at(clappkit::paths::data_file(crate::CLI, "crm.json"))
    }
}

impl CrmStore for JsonStore {
    /// Read the dataset, or start a fresh one.
    ///
    /// Three outcomes, and the difference between them is printed because only one of them
    /// is a first run:
    ///
    /// * **no file** — a first run. A seeded [`Db`], silently.
    /// * **a damaged file** — moved aside as `crm.json.corrupt-<epoch>` and a fresh Db
    ///   returned. Refusing to start would leave the person with no app *and* no way to
    ///   reach their data; overwriting it would leave them with no data at all. Neither is
    ///   a choice worth making on their behalf, so the file is kept and named.
    /// * **a good file** — parsed.
    fn load(&self) -> Result<Db> {
        let bytes = match std::fs::read(&self.path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Db::default()),
            Err(e) => {
                return Err(e).with_context(|| format!("cannot read {}", self.path.display()))
            }
        };

        match serde_json::from_slice::<Db>(&bytes) {
            Ok(db) => Ok(db),
            Err(e) => {
                let kept = clappkit::store::quarantine(&self.path);
                eprintln!(
                    "crm: {} would not parse ({e}) — starting fresh. Your data is still there{}",
                    self.path.display(),
                    match &kept {
                        Some(p) => format!(", at {}", p.display()),
                        None => String::new(),
                    }
                );
                Ok(Db::default())
            }
        }
    }

    /// Atomically, or not at all. A half-written CRM is a lost CRM: `atomic_write` stages
    /// beside the target, fsyncs, renames over it, then fsyncs the directory — so a power
    /// loss leaves either the old file or the new one, never a truncated one.
    fn save(&self, db: &Db) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(db).context("cannot serialise the dataset")?;
        clappkit::store::atomic_write(&self.path, &bytes)
            .with_context(|| format!("cannot write {}", self.path.display()))
    }
}

/// This install's own identity, minted once and kept beside the data.
///
/// **Its own file, not a field in the dataset.** The dataset is the thing that will one
/// day be synced; if the origin travelled inside it, every install that received a copy
/// would claim to be the install that wrote it, and the field would say nothing at all.
///
/// `fresh` supplies the randomness rather than this function reading any, so a test can
/// pin exactly what a given draw produces.
pub fn load_or_mint_instance_id(
    path: &std::path::Path,
    fresh: impl FnOnce() -> [u8; 16],
) -> Result<InstanceId> {
    if let Ok(existing) = std::fs::read_to_string(path) {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            return Ok(InstanceId(trimmed.to_string()));
        }
    }
    let minted = InstanceId::from_bytes(fresh());
    clappkit::store::atomic_write(path, minted.as_str().as_bytes())
        .with_context(|| format!("cannot write {}", path.display()))?;
    Ok(minted)
}

/// A debounced writer in front of a [`CrmStore`].
///
/// **A card dragged across a board is one save, not sixty.** The port itself stays
/// timing-free — `save` means "write this now", which is what makes it implementable by
/// anything — and the coalescing lives here, where the runtime is.
///
/// Latest wins: this holds one pending dataset, not a queue of them, because writing an
/// intermediate state that was never on screen for longer than a frame buys nothing.
pub struct SaveQueue {
    tx: tokio::sync::mpsc::Sender<Db>,
}

impl SaveQueue {
    /// The queue, and the writer loop that drains it.
    ///
    /// The loop is **returned rather than spawned**, because this module has no business
    /// choosing a runtime: the GUI runs on Tauri's (`tauri::async_runtime::spawn`), and a
    /// bare `tokio::spawn` from Tauri's `setup` — which is the main thread, outside any
    /// runtime context — panics.
    ///
    /// `quiet` is how long the dataset must go unchanged before it is written: long enough
    /// to swallow a card dragged across a board, short enough that a crash costs one
    /// gesture.
    pub fn channel(
        store: Arc<dyn CrmStore>,
        quiet: Duration,
    ) -> (SaveQueue, impl std::future::Future<Output = ()> + Send + 'static) {
        // A depth of one is all that is meaningful when latest-wins, but a little slack
        // keeps a burst of writes from ever blocking the command that produced them.
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Db>(32);
        let writer = async move {
            while let Some(mut latest) = rx.recv().await {
                // Keep taking whatever arrives until the dataset goes quiet, then write
                // once. A closed channel means the app is going away — write immediately
                // rather than waiting out a timer nobody is left to satisfy.
                while let Ok(Some(newer)) = tokio::time::timeout(quiet, rx.recv()).await {
                    latest = newer;
                }
                if let Err(e) = store.save(&latest) {
                    // Persistence is best-effort and never a panic: a full disk must not
                    // take the window down. It must not be silent either — this is the
                    // line that distinguishes "we saved nothing" from "nothing changed".
                    eprintln!("crm: save failed: {e:#}");
                }
            }
        };
        (SaveQueue { tx }, writer)
    }

    /// Queue a dataset to be written once the changes stop.
    pub fn save(&self, db: Db) {
        // Full means the writer is already behind with newer data on the way; dropping
        // this one loses nothing, because the next send carries the same state and more.
        let _ = self.tx.try_send(db);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Deal, Money, Stage, Status, Ulid};
    use std::path::Path;

    /// A real ULID, minted by the same code the app uses.
    ///
    /// Fixtures used to spell ids by hand — `01J0DEAL0…`, `01J0COMPANY0…` — which look
    /// like ULIDs and are not: Crockford base32 has no `L`, `O`, `I` or `U`, so neither
    /// string could ever have come out of the minter. A fixture the code could not have
    /// produced is a test measuring something that never happens.
    fn an_id(seed: u8) -> String {
        Ulid::from_parts(1_700_000_000_000, [seed; 10]).to_string()
    }

    /// A scratch path that no other test — and no person's real data directory — shares.
    fn scratch(name: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir()
            .join(format!("crm-store-{}-{}-{name}", std::process::id(), n))
            .join("crm.json")
    }

    fn cleanup(path: &Path) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    fn a_deal(id: &str, pipeline: &str) -> Deal {
        Deal {
            id: id.into(),
            handle: "acme-renewal".into(),
            title: "Acme renewal".into(),
            company_id: Some(an_id(0xC0)),
            contact_ids: vec!["ada".into()],
            value: Some(Money::new(4_500_000, "USD")),
            stage: Stage::Negotiation,
            status: Status::Open,
            pipeline_id: pipeline.into(),
            opened_at: 1_700_000_000_000,
            closed_at: None,
            moved_by: crate::model::Actor::Agent { id: "agent-7".into() },
            moved_at: 1_700_000_000_000,
            archived_at: None,
            updated_at: 1_700_000_000_000,
            origin: InstanceId::default(),
        }
    }

    /// **The M1 seam test.** `pipeline_id` is a field nothing reads in v1, and an untested
    /// seam is usually a subtly wrong one. If it does not survive a write and a read, the
    /// structural half of multi-pipeline support is decoration.
    #[test]
    fn pipeline_id_survives_a_save_and_a_load() {
        let path = scratch("pipeline-id");
        let store = JsonStore::at(&path);

        let mut db = Db::default();
        db.deals.push(a_deal(&an_id(1), "sales"));
        store.save(&db).unwrap();

        let back = store.load().unwrap();
        assert_eq!(back.deals.len(), 1);
        assert_eq!(back.deals[0].pipeline_id, "sales", "the seam is decoration if this drifts");
        assert_eq!(back.pipelines.len(), 1);
        assert_eq!(back.pipelines[0].id, "sales");
        cleanup(&path);
    }

    /// Not just that one field: everything the core will ever hand the store has to come
    /// back identical, or a restart is a quiet data loss.
    #[test]
    fn a_whole_dataset_round_trips_unchanged() {
        let path = scratch("whole");
        let store = JsonStore::at(&path);

        let mut db = Db::default();
        db.deals.push(a_deal(&an_id(1), "sales"));
        db.deals.push({
            let mut d = a_deal(&an_id(2), "sales");
            d.status = Status::Won;
            d.closed_at = Some(1_700_000_100_000);
            d.value = None;
            d
        });
        store.save(&db).unwrap();

        assert_eq!(store.load().unwrap(), db);
        cleanup(&path);
    }

    #[test]
    fn a_first_run_has_no_file_and_is_not_an_error() {
        let path = scratch("first-run");
        let db = JsonStore::at(&path).load().unwrap();
        assert_eq!(db, Db::default());
        assert_eq!(db.pipelines.len(), 1, "the one pipeline is seeded at first run");
        cleanup(&path);
    }

    /// A damaged file must not take the app down, and must not be overwritten either. The
    /// person keeps their bytes and gets a working app; both, or neither is much use.
    #[test]
    fn a_damaged_file_is_kept_aside_rather_than_overwritten() {
        let path = scratch("corrupt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{ this is not json").unwrap();

        let db = JsonStore::at(&path).load().unwrap();
        assert_eq!(db, Db::default(), "the app starts, on a fresh dataset");

        let dir = path.parent().unwrap();
        let kept: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("corrupt-"))
            .collect();
        assert_eq!(kept.len(), 1, "the damaged file is still on disk, under a new name");
        cleanup(&path);
    }

    /// An older dataset must open in a newer build. Every collection defaults, so a file
    /// written before a field existed is read rather than refused.
    #[test]
    fn a_dataset_missing_fields_is_read_not_refused() {
        let path = scratch("sparse");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, br#"{"companies":[]}"#).unwrap();

        let db = JsonStore::at(&path).load().unwrap();
        assert!(db.deals.is_empty());
        assert_eq!(db.pipelines.len(), 1, "a missing pipeline list is seeded, not empty");
        cleanup(&path);
    }

    /// The store writes through `clappkit::store::atomic_write`, which creates the parent
    /// directory — a first save must not fail because nothing has made the folder yet.
    #[test]
    fn the_first_save_creates_its_own_directory() {
        let path = scratch("mkdir");
        assert!(!path.parent().unwrap().exists());
        JsonStore::at(&path).save(&Db::default()).unwrap();
        assert!(path.exists());
        cleanup(&path);
    }

    /// The install's identity is minted once and then never moves. If it changed on every
    /// launch, `origin` would say "some run of this app" rather than "this install", and
    /// conflict resolution would have nothing to resolve against.
    #[test]
    fn an_instance_id_is_minted_once_and_then_read_back_forever() {
        let path = scratch("instance").parent().unwrap().join("instance");

        let first = load_or_mint_instance_id(&path, || [7; 16]).unwrap();
        assert_eq!(first, InstanceId::from_bytes([7; 16]));

        // A second call with *different* randomness must still return the first id.
        let second = load_or_mint_instance_id(&path, || [9; 16]).unwrap();
        assert_eq!(second, first, "the identity is minted once, not once per launch");

        cleanup(&path);
    }

    /// A truncated or blank file is not an identity. Minting a fresh one is right; reading
    /// an empty string back as this install's name is not.
    #[test]
    fn a_blank_instance_file_is_re_minted_rather_than_believed() {
        let path = scratch("blank-instance").parent().unwrap().join("instance");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "   \n").unwrap();

        let id = load_or_mint_instance_id(&path, || [3; 16]).unwrap();
        assert_eq!(id, InstanceId::from_bytes([3; 16]));
        assert!(!id.as_str().is_empty());
        cleanup(&path);
    }

    /// **The M1-revision seam test.** `origin` and `updated_at` are read by nothing in v1,
    /// and an untested seam is usually a subtly wrong one — this is the whole reason they
    /// exist now rather than later.
    #[test]
    fn origin_and_updated_at_survive_a_save_and_a_load() {
        let path = scratch("origin");
        let store = JsonStore::at(&path);
        let origin = InstanceId::from_bytes([5; 16]);

        let mut db = Db::default();
        let mut deal = a_deal(&an_id(3), "sales");
        deal.origin = origin.clone();
        deal.updated_at = 1_700_000_500_000;
        db.deals.push(deal);
        db.companies.push(crate::model::Company {
            id: an_id(4),
            handle: "acme".into(),
            name: "Acme Corp".into(),
            domain: None,
            tags: Vec::new(),
            notes: None,
            archived_at: None,
            updated_at: 1_700_000_400_000,
            origin: origin.clone(),
        });
        store.save(&db).unwrap();

        let back = store.load().unwrap();
        assert_eq!(back.deals[0].origin, origin, "which install wrote this version");
        assert_eq!(back.deals[0].updated_at, 1_700_000_500_000);
        assert_eq!(back.companies[0].origin, origin);
        assert_eq!(back.companies[0].updated_at, 1_700_000_400_000);
        assert_eq!(back.companies[0].handle, "acme", "and the handle beside the id");
        cleanup(&path);
    }
}
