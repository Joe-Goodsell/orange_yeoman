// SQLite-backed concept store (research doc section 2.3) plus the queue it
// feeds. The store is a graceful-degradation singleton in Tauri state: if the
// DB cannot open at startup, the connection stays None and every store call
// no-ops so the app keeps running without a concept index.
//
// Schema v1 (blocks, concepts, occurrences, links, embeddings, pending_work)
// is managed by PRAGMA user_version with an in-code migration list. Blocks are
// upserted from the incremental diff (see incremental.rs); concepts and
// occurrences are written by later pipeline stages (PER-33+) and only
// maintained here (cascade delete + orphan cleanup).

use crate::incremental::{BlockChange, FileDiff, NewBlock, StoredBlock};
use crate::pipeline::{
    block_kind_as_str, parse_slash_command_in_block, MarkdownBlock, SlashCommand,
};
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Filename of the concept store DB, next to the global config in the app data
/// directory.
pub(crate) const DB_FILE_NAME: &str = "concept_store.db";

/// Schema v1. `IF NOT EXISTS` everywhere so a re-run of the migration list is
/// harmless; the user_version guard normally skips already-applied versions.
const SCHEMA_V1: &str = "
CREATE TABLE IF NOT EXISTS blocks (
    id INTEGER PRIMARY KEY,
    file_path TEXT NOT NULL,
    block_hash TEXT NOT NULL,
    heading_path TEXT NOT NULL,
    char_start INTEGER NOT NULL,
    char_end INTEGER NOT NULL,
    kind TEXT NOT NULL,
    excluded INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS concepts (
    id INTEGER PRIMARY KEY,
    type TEXT NOT NULL,
    normalized_name TEXT NOT NULL,
    first_seen INTEGER NOT NULL,
    UNIQUE(type, normalized_name)
);
CREATE TABLE IF NOT EXISTS occurrences (
    id INTEGER PRIMARY KEY,
    concept_id INTEGER NOT NULL REFERENCES concepts(id) ON DELETE CASCADE,
    block_id INTEGER NOT NULL REFERENCES blocks(id) ON DELETE CASCADE,
    start INTEGER NOT NULL,
    end INTEGER NOT NULL,
    surface TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS links (
    id INTEGER PRIMARY KEY,
    source_block_id INTEGER NOT NULL REFERENCES blocks(id) ON DELETE CASCADE,
    target_path TEXT NOT NULL,
    target_heading TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS embeddings (
    block_hash TEXT NOT NULL,
    model_id TEXT NOT NULL,
    vector BLOB NOT NULL,
    UNIQUE(block_hash, model_id)
);
CREATE TABLE IF NOT EXISTS pending_work (
    id INTEGER PRIMARY KEY,
    file_path TEXT NOT NULL,
    block_hash TEXT NOT NULL,
    reason TEXT NOT NULL CHECK(reason IN ('added','changed')),
    status TEXT NOT NULL DEFAULT 'pending',
    enqueued_at INTEGER NOT NULL,
    UNIQUE(file_path, block_hash)
);
CREATE INDEX IF NOT EXISTS idx_blocks_file_path ON blocks(file_path);
CREATE INDEX IF NOT EXISTS idx_blocks_block_hash ON blocks(block_hash);
CREATE INDEX IF NOT EXISTS idx_occurrences_block_id ON occurrences(block_id);
CREATE INDEX IF NOT EXISTS idx_occurrences_concept_id ON occurrences(concept_id);
CREATE INDEX IF NOT EXISTS idx_links_source_block_id ON links(source_block_id);
";

const MIGRATIONS: [&str; 1] = [SCHEMA_V1];

/// Managed store state. `conn` is None when the DB could not be opened;
/// every operation then returns early. `path_locks` serializes same-file
/// processing so concurrent tasks for one path converge to the latest content.
pub(crate) struct StoreState {
    conn: Mutex<Option<Connection>>,
    path_locks: Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>,
}

impl Default for StoreState {
    fn default() -> Self {
        StoreState::new()
    }
}

impl StoreState {
    pub(crate) fn new() -> Self {
        StoreState {
            conn: Mutex::new(None),
            path_locks: Mutex::new(HashMap::new()),
        }
    }

    /// Open the DB (creating the data dir if needed) and run migrations.
    /// Logs errors and leaves the connection None on failure.
    pub(crate) fn open(&self) {
        let mut guard = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        *guard = match open_database() {
            Ok(conn) => Some(conn),
            Err(e) => {
                tracing::error!(
                    category = "store",
                    "concept store unavailable ({}); processing is disabled",
                    e
                );
                None
            }
        };
    }

    /// Run a closure against the store connection. Returns None when the DB is
    /// unavailable or the mutex is poisoned, so callers degrade gracefully.
    pub(crate) fn with_conn<T>(&self, f: impl FnOnce(&mut Connection) -> T) -> Option<T> {
        let mut guard = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_mut().map(f)
    }

    /// Per-path serialization handle. The map lock is held only to clone the
    /// Arc; the returned mutex is held by the caller for the whole processing
    /// of that path.
    pub(crate) fn path_lock(&self, path: &Path) -> Arc<Mutex<()>> {
        let mut map = self.path_locks.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(path.to_path_buf())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

/// Open the concept store DB next to the global config and prepare it for use.
/// The error is a String because it is only ever logged in `StoreState::open`.
fn open_database() -> Result<Connection, String> {
    let dir = crate::config::app_data_dir()
        .ok_or_else(|| "cannot resolve app data dir".to_string())?;
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create data dir {}: {e}", dir.display()))?;
    let conn = Connection::open(dir.join(DB_FILE_NAME))
        .map_err(|e| format!("cannot open {}: {e}", DB_FILE_NAME))?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| format!("cannot set WAL journal mode: {e}"))?;
    migrate(&conn).map_err(|e| format!("cannot migrate schema: {e}"))?;
    Ok(conn)
}

/// Apply pending schema migrations. Idempotent: the user_version guard skips
/// versions already applied, and each statement uses IF NOT EXISTS anyway.
fn migrate(conn: &Connection) -> Result<(), rusqlite::Error> {
    // Foreign keys are enforced per connection; the migration sets them so
    // in-memory test connections get the same behavior as the real DB.
    conn.pragma_update(None, "foreign_keys", "ON")?;
    let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    for (index, sql) in MIGRATIONS.iter().enumerate() {
        let version = index as i64 + 1;
        if version > current {
            conn.execute_batch(sql)?;
            conn.pragma_update(None, "user_version", version)?;
        }
    }
    Ok(())
}

/// Load one file's stored blocks ordered by start offset.
pub(crate) fn load_stored_blocks(conn: &Connection, file_path: &Path) -> Vec<StoredBlock> {
    let path_str = file_path.to_string_lossy();
    let mut stmt = match conn.prepare(
        "SELECT id, file_path, block_hash, heading_path, char_start, char_end, kind, excluded \
         FROM blocks WHERE file_path = ?1 ORDER BY char_start",
    ) {
        Ok(stmt) => stmt,
        Err(e) => {
            tracing::error!(category = "store", "load_stored_blocks prepare failed: {e}");
            return Vec::new();
        }
    };
    stmt.query_map(params![path_str.as_ref()], |row| {
        Ok(StoredBlock {
            id: row.get(0)?,
            file_path: row.get(1)?,
            block_hash: row.get(2)?,
            heading_path: row.get(3)?,
            char_start: row.get::<_, i64>(4)? as usize,
            char_end: row.get::<_, i64>(5)? as usize,
            kind: row.get(6)?,
            excluded: row.get::<_, i64>(7)? != 0,
        })
    })
    .map(|rows| rows.flatten().collect())
    .unwrap_or_default()
}

/// Insert one block row and return its id.
fn insert_block(conn: &Connection, file_path: &Path, new: &NewBlock) -> Result<i64, rusqlite::Error> {
    conn.execute(
        "INSERT INTO blocks (file_path, block_hash, heading_path, char_start, char_end, kind, excluded) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            file_path.to_string_lossy().as_ref(),
            new.block_hash,
            new.heading_path,
            new.char_start as i64,
            new.char_end as i64,
            block_kind_as_str(&new.kind),
            new.excluded as i64,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Refresh the position and heading of an existing block row. Offsets shift
/// after edits above the block, so this runs for Unchanged and Moved blocks.
fn update_block_position(
    conn: &Connection,
    id: i64,
    char_start: usize,
    char_end: usize,
    heading_path: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE blocks SET char_start = ?1, char_end = ?2, heading_path = ?3 WHERE id = ?4",
        params![char_start as i64, char_end as i64, heading_path, id],
    )?;
    Ok(())
}

/// Delete one block row. Occurrences and links cascade.
fn delete_block(conn: &Connection, id: i64) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM blocks WHERE id = ?1", params![id])?;
    Ok(())
}

/// Delete concepts that no longer have any occurrence anywhere. Shared
/// concepts (occurrences in surviving blocks) are untouched; this is the
/// re-link rule from research section 2.2 step 6.
fn delete_orphan_concepts(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute(
        "DELETE FROM concepts WHERE id NOT IN (SELECT DISTINCT concept_id FROM occurrences)",
        [],
    )?;
    Ok(())
}

/// Enqueue a block for later stages. ON CONFLICT resets an existing queue row
/// to pending and refreshes the reason and timestamp.
fn enqueue_pending(
    conn: &Connection,
    file_path: &Path,
    block_hash: &str,
    reason: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO pending_work (file_path, block_hash, reason, status, enqueued_at) \
         VALUES (?1, ?2, ?3, 'pending', ?4) \
         ON CONFLICT(file_path, block_hash) DO UPDATE SET \
             status = 'pending', reason = excluded.reason, enqueued_at = excluded.enqueued_at",
        params![file_path.to_string_lossy().as_ref(), block_hash, reason, now_millis()],
    )?;
    Ok(())
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Enqueue filter: excluded blocks (front matter, code fences, HTML) and
/// blocks carrying an `/ignore` slash command are stored but never enqueued.
fn should_enqueue(new: &NewBlock) -> bool {
    if new.excluded {
        return false;
    }
    // Re-run the existing slash-command parser over the block text. Only the
    // /ignore command suppresses queueing; fact-check and research blocks
    // still enqueue (band routing is out of scope for this ticket).
    let block = MarkdownBlock {
        kind: new.kind.clone(),
        text: new.text.clone(),
        start: 0,
        end: 0,
        heading_chain: Vec::new(),
        excluded: new.excluded,
        block_hash: new.block_hash.clone(),
    };
    match parse_slash_command_in_block(&block) {
        Some(parsed) => parsed.command != SlashCommand::Ignore,
        None => true,
    }
}

/// Outcome of one apply pass, used for the pipeline debug-console summary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ApplyStats {
    pub(crate) stored: usize,
    pub(crate) updated: usize,
    pub(crate) deleted: usize,
    pub(crate) enqueued: usize,
}

/// Apply a file diff to the store in one transaction:
/// - Unchanged/Moved: refresh char_start/char_end/heading_path by id; no
///   enqueue (concepts stay attached to the moved row).
/// - Removed: delete the row (occurrences cascade); orphan concepts are
///   cleaned once after all deletions.
/// - Changed: delete the old row, insert the new one, enqueue 'changed'.
/// - Added: insert the row, enqueue 'added'.
/// Excluded and /ignore blocks are stored but not enqueued. On any DB error
/// the transaction rolls back and an error is logged; the app keeps running.
pub(crate) fn apply_diff(conn: &mut Connection, path: &Path, diff: &FileDiff) -> ApplyStats {
    let mut stats = ApplyStats::default();
    let tx = match conn.transaction() {
        Ok(tx) => tx,
        Err(e) => {
            tracing::error!(category = "store", "apply_diff: cannot start transaction: {e}");
            return stats;
        }
    };

    let mut deleted_any = false;
    for change in &diff.changes {
        let result = match change {
            BlockChange::Unchanged { stored, new } | BlockChange::Moved { stored, new } => {
                stats.updated += 1;
                update_block_position(&tx, stored.id, new.char_start, new.char_end, &new.heading_path)
            }
            BlockChange::Removed { stored } => {
                deleted_any = true;
                stats.deleted += 1;
                delete_block(&tx, stored.id)
            }
            BlockChange::Changed { old, new } => {
                deleted_any = true;
                stats.deleted += 1;
                if let Err(e) = delete_block(&tx, old.id) {
                    return apply_error(stats, e);
                }
                if let Err(e) = insert_block(&tx, path, new) {
                    return apply_error(stats, e);
                }
                stats.stored += 1;
                if should_enqueue(new) {
                    if let Err(e) = enqueue_pending(&tx, path, &new.block_hash, "changed") {
                        return apply_error(stats, e);
                    }
                    stats.enqueued += 1;
                }
                continue;
            }
            BlockChange::Added { new } => {
                if let Err(e) = insert_block(&tx, path, new) {
                    return apply_error(stats, e);
                }
                stats.stored += 1;
                if should_enqueue(new) {
                    if let Err(e) = enqueue_pending(&tx, path, &new.block_hash, "added") {
                        return apply_error(stats, e);
                    }
                    stats.enqueued += 1;
                }
                continue;
            }
        };
        if let Err(e) = result {
            return apply_error(stats, e);
        }
    }

    if deleted_any {
        if let Err(e) = delete_orphan_concepts(&tx) {
            return apply_error(stats, e);
        }
    }
    if let Err(e) = tx.commit() {
        return apply_error(stats, e);
    }
    stats
}

fn apply_error(stats: ApplyStats, e: rusqlite::Error) -> ApplyStats {
    tracing::error!(category = "store", "apply_diff failed, rolled back: {e}");
    stats
}

/// Remove every stored block for a path (file deletion / rename fallback).
/// Occurrences cascade; orphan concepts are cleaned afterwards.
pub(crate) fn remove_all_blocks_for_path(conn: &mut Connection, path: &Path) {
    let result = (|| -> Result<(), rusqlite::Error> {
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM blocks WHERE file_path = ?1",
            params![path.to_string_lossy().as_ref()],
        )?;
        delete_orphan_concepts(&tx)?;
        tx.commit()?;
        Ok(())
    })();
    if let Err(e) = result {
        tracing::error!(category = "store", "remove_all_blocks_for_path failed: {e}");
    }
}

/// Re-link a renamed file: point every block row at the new path. Concepts and
/// occurrences stay attached; no re-extraction happens. Returns the number of
/// rows updated.
pub(crate) fn relink_blocks(conn: &Connection, from: &Path, to: &Path) -> Result<usize, rusqlite::Error> {
    conn.execute(
        "UPDATE blocks SET file_path = ?1 WHERE file_path = ?2",
        params![to.to_string_lossy().as_ref(), from.to_string_lossy().as_ref()],
    )
}

/// The testable processing core: parse, diff, apply. Returns the diff and the
/// apply stats so callers can log a summary. `content` is supplied by the
/// caller (the watcher reads the file under the per-path lock; tests pass
/// content directly).
pub(crate) fn process_file_content(
    conn: &mut Connection,
    path: &Path,
    content: &str,
) -> (FileDiff, ApplyStats) {
    let blocks = crate::pipeline::parse_markdown_blocks(content);
    let new: Vec<NewBlock> = blocks.iter().map(NewBlock::from_block).collect();
    let stored = load_stored_blocks(conn, path);
    let diff = crate::incremental::diff_block_lists(&stored, &new);
    let stats = apply_diff(conn, path, &diff);
    (diff, stats)
}

#[cfg(test)]
mod tests;