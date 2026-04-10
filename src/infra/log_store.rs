use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::Utc;
use rusqlite::{Connection, params};

use crate::models::LogEvent;

// ---------------------------------------------------------------------------
// StoredEvent
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct StoredEvent {
    pub id: i64,
    pub event_type: String,
    pub payload_json: String,
    pub timestamp: String,
}

// ---------------------------------------------------------------------------
// LogStore
// ---------------------------------------------------------------------------

pub struct LogStore {
    conn: Connection,
}

impl LogStore {
    /// Open or create the SQLite database at `path`, enable WAL mode, and
    /// create the events table if it does not already exist.
    pub fn new(path: &Path) -> Result<Self> {
        // Create parent directories if needed.
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;

        // Enable WAL journal mode.
        conn.pragma_update(None, "journal_mode", "WAL")?;

        // Set busy timeout so concurrent writers retry instead of failing immediately.
        conn.busy_timeout(std::time::Duration::from_secs(5))?;

        // Create the events table.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS events (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                event_type   TEXT    NOT NULL,
                payload_json TEXT    NOT NULL,
                timestamp    TEXT    NOT NULL
            );",
        )?;

        Ok(Self { conn })
    }

    /// Returns the default database path: `~/.local/share/tmuxbar/logs.db`.
    pub fn default_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".local/share/tmuxbar/logs.db")
    }

    /// Serialize `event` and INSERT it into the events table.
    pub fn insert_event(&self, event: &LogEvent) -> Result<()> {
        let event_type = event_type_str(event);
        let payload_json = serde_json::to_string(event)?;
        let timestamp = Utc::now().to_rfc3339();

        self.conn.execute(
            "INSERT INTO events (event_type, payload_json, timestamp) VALUES (?1, ?2, ?3)",
            params![event_type, payload_json, timestamp],
        )?;

        Ok(())
    }

    /// Query events from the database.
    ///
    /// - `event_type`: when `Some`, only rows with a matching `event_type` are
    ///   returned; when `None`, all event types are included.
    /// - `limit`: maximum number of rows to return (newest first).
    pub fn query_events(
        &self,
        event_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<StoredEvent>> {
        let rows = match event_type {
            Some(et) => {
                let mut stmt = self.conn.prepare(
                    "SELECT id, event_type, payload_json, timestamp
                     FROM events
                     WHERE event_type = ?1
                     ORDER BY id DESC
                     LIMIT ?2",
                )?;
                let iter = stmt.query_map(params![et, limit as i64], row_to_stored_event)?;
                iter.collect::<rusqlite::Result<Vec<_>>>()?
            }
            None => {
                let mut stmt = self.conn.prepare(
                    "SELECT id, event_type, payload_json, timestamp
                     FROM events
                     ORDER BY id DESC
                     LIMIT ?1",
                )?;
                let iter = stmt.query_map(params![limit as i64], row_to_stored_event)?;
                iter.collect::<rusqlite::Result<Vec<_>>>()?
            }
        };

        Ok(rows)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn event_type_str(event: &LogEvent) -> &'static str {
    match event {
        LogEvent::FdSpike { .. } => "FdSpike",
        LogEvent::SessionCreated { .. } => "SessionCreated",
        LogEvent::SessionDestroyed { .. } => "SessionDestroyed",
        LogEvent::SafeRestart { .. } => "SafeRestart",
    }
}

fn row_to_stored_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredEvent> {
    Ok(StoredEvent {
        id: row.get(0)?,
        event_type: row.get(1)?,
        payload_json: row.get(2)?,
        timestamp: row.get(3)?,
    })
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::RestartPhase;
    use chrono::Utc;
    use tempfile::tempdir;

    fn open_store() -> (LogStore, tempfile::TempDir) {
        let dir = tempdir().expect("tempdir");
        let store = LogStore::new(&dir.path().join("test.db")).expect("LogStore::new");
        (store, dir)
    }

    // ------------------------------------------------------------------
    // Test 1: insert a FdSpike and query it back
    // ------------------------------------------------------------------
    #[test]
    fn insert_and_query_fd_spike() {
        let (store, _dir) = open_store();

        let event = LogEvent::FdSpike {
            pct: 72,
            timestamp: Utc::now(),
        };
        store.insert_event(&event).expect("insert_event");

        let rows = store.query_events(None, 10).expect("query_events");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].event_type, "FdSpike");
        assert!(rows[0].payload_json.contains("FdSpike"));
        assert!(rows[0].payload_json.contains("72"));
        assert!(!rows[0].timestamp.is_empty());
    }

    // ------------------------------------------------------------------
    // Test 2: insert multiple events and verify limit
    // ------------------------------------------------------------------
    #[test]
    fn query_respects_limit() {
        let (store, _dir) = open_store();

        for i in 0..5u8 {
            store
                .insert_event(&LogEvent::FdSpike {
                    pct: i * 10,
                    timestamp: Utc::now(),
                })
                .expect("insert");
        }

        let rows = store.query_events(None, 3).expect("query_events");
        assert_eq!(rows.len(), 3);

        // Verify newest-first ordering: the last inserted event has the
        // highest id and should be first.
        assert!(rows[0].id > rows[1].id);
        assert!(rows[1].id > rows[2].id);
    }

    // ------------------------------------------------------------------
    // Test 3: filter by event_type
    // ------------------------------------------------------------------
    #[test]
    fn filter_by_event_type() {
        let (store, _dir) = open_store();

        store
            .insert_event(&LogEvent::SessionCreated {
                name: "work".into(),
            })
            .expect("insert SessionCreated");

        store
            .insert_event(&LogEvent::SessionDestroyed {
                name: "old".into(),
            })
            .expect("insert SessionDestroyed");

        store
            .insert_event(&LogEvent::SafeRestart {
                phase: RestartPhase::ServerKill,
                success: true,
            })
            .expect("insert SafeRestart");

        // Filter for SessionCreated only
        let created = store
            .query_events(Some("SessionCreated"), 10)
            .expect("query SessionCreated");
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].event_type, "SessionCreated");
        assert!(created[0].payload_json.contains("work"));

        // Filter for SessionDestroyed only
        let destroyed = store
            .query_events(Some("SessionDestroyed"), 10)
            .expect("query SessionDestroyed");
        assert_eq!(destroyed.len(), 1);
        assert_eq!(destroyed[0].event_type, "SessionDestroyed");

        // Filter for SafeRestart
        let restarts = store
            .query_events(Some("SafeRestart"), 10)
            .expect("query SafeRestart");
        assert_eq!(restarts.len(), 1);
        assert_eq!(restarts[0].event_type, "SafeRestart");

        // Non-existent type returns empty
        let none = store
            .query_events(Some("NonExistent"), 10)
            .expect("query NonExistent");
        assert!(none.is_empty());
    }

    // ------------------------------------------------------------------
    // Test 4: WAL mode is enabled
    // ------------------------------------------------------------------
    #[test]
    fn wal_mode_is_enabled() {
        let (store, _dir) = open_store();

        let mode: String = store
            .conn
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .expect("pragma journal_mode");

        assert_eq!(mode, "wal", "journal_mode should be 'wal'");
    }

    // ------------------------------------------------------------------
    // Test 5: empty query returns empty vec
    // ------------------------------------------------------------------
    #[test]
    fn empty_store_returns_empty_vec() {
        let (store, _dir) = open_store();

        let rows = store.query_events(None, 100).expect("query_events");
        assert!(rows.is_empty());

        let rows_filtered = store
            .query_events(Some("FdSpike"), 100)
            .expect("query_events filtered");
        assert!(rows_filtered.is_empty());
    }
}
