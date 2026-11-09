use super::model::{Download, DownloadState, Search, SearchHit};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use std::{path::Path, time::Duration};

pub const MAX_RESULTS: usize = 5000;
pub const MAX_JOBS: usize = 1024;

pub struct Store(Connection);

fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
pub fn now() -> String {
    Utc::now().to_rfc3339()
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(error)?;
        Self::initialize(conn)
    }

    fn initialize(conn: Connection) -> Result<Self, String> {
        conn.busy_timeout(Duration::from_secs(5)).map_err(error)?;
        conn.execute_batch("
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS soulseek_searches (
                id TEXT PRIMARY KEY, server TEXT NOT NULL, account TEXT NOT NULL,
                query TEXT NOT NULL, state TEXT NOT NULL, created_at TEXT NOT NULL,
                last_error TEXT
            );
            CREATE TABLE IF NOT EXISTS soulseek_results (
                id TEXT PRIMARY KEY, search_id TEXT NOT NULL REFERENCES soulseek_searches(id) ON DELETE CASCADE,
                username TEXT NOT NULL, remote_path TEXT NOT NULL, file_json TEXT NOT NULL,
                slots_free INTEGER NOT NULL, average_speed INTEGER NOT NULL, queue_length INTEGER NOT NULL,
                UNIQUE(search_id, username, remote_path)
            );
            CREATE TABLE IF NOT EXISTS soulseek_downloads (
                id TEXT PRIMARY KEY, batch_id TEXT, server TEXT NOT NULL, account TEXT NOT NULL,
                username TEXT NOT NULL, remote_path TEXT NOT NULL, expected_size INTEGER NOT NULL CHECK(expected_size >= 0),
                file_json TEXT NOT NULL, destination TEXT NOT NULL, output_root TEXT NOT NULL, remote_root TEXT NOT NULL,
                state TEXT NOT NULL, bytes INTEGER NOT NULL CHECK(bytes >= 0), attempts INTEGER NOT NULL,
                last_error TEXT, checkpoint_json TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS soulseek_live_destination ON soulseek_downloads(lower(destination))
                WHERE state NOT IN ('completed', 'cancelled', 'failed');
            CREATE UNIQUE INDEX IF NOT EXISTS soulseek_live_source ON soulseek_downloads(server, account, username, remote_path)
                WHERE state NOT IN ('completed', 'cancelled', 'failed');
            CREATE INDEX IF NOT EXISTS soulseek_results_search ON soulseek_results(search_id);
        ").map_err(error)?;
        Ok(Self(conn))
    }

    pub fn recover(&mut self) -> Result<(), String> {
        let tx = self.0.transaction().map_err(error)?;
        tx.execute(
            "UPDATE soulseek_searches SET state = 'interrupted' WHERE state = 'searching'",
            [],
        )
        .map_err(error)?;
        tx.execute("UPDATE soulseek_downloads SET state = 'interrupted', updated_at = ?1 WHERE state IN ('queued', 'running', 'retry_waiting')", [now()]).map_err(error)?;
        tx.commit().map_err(error)
    }

    pub fn searches(&self) -> Result<Vec<Search>, String> {
        let mut stmt = self.0.prepare("SELECT s.id, s.server, s.account, s.query, s.state, s.created_at, s.last_error,
            (SELECT count(*) FROM soulseek_results r WHERE r.search_id = s.id) FROM soulseek_searches s ORDER BY s.created_at DESC").map_err(error)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Search {
                    id: r.get(0)?,
                    server: r.get(1)?,
                    account: r.get(2)?,
                    query: r.get(3)?,
                    state: r.get(4)?,
                    created_at: r.get(5)?,
                    last_error: r.get(6)?,
                    result_count: r.get(7)?,
                })
            })
            .map_err(error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(error)
    }

    pub fn search(&self, id: &str) -> Result<Search, String> {
        self.searches()?
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| "Búsqueda no encontrada.".into())
    }

    pub fn insert_search(&self, search: &Search) -> Result<(), String> {
        self.0.execute("INSERT INTO soulseek_searches (id,server,account,query,state,created_at) VALUES (?1,?2,?3,?4,?5,?6)", params![search.id,search.server,search.account,search.query,search.state,search.created_at]).map_err(error)?;
        Ok(())
    }

    pub fn finish_search(&self, id: &str, state: &str, reason: Option<&str>) -> Result<(), String> {
        self.0
            .execute(
                "UPDATE soulseek_searches SET state = ?2, last_error = ?3 WHERE id = ?1",
                params![id, state, reason],
            )
            .map_err(error)?;
        Ok(())
    }

    pub fn delete_search(&self, id: &str) -> Result<(), String> {
        self.0
            .execute("DELETE FROM soulseek_searches WHERE id = ?1", [id])
            .map_err(error)?;
        Ok(())
    }

    pub fn save_hits(&mut self, id: &str, hits: &[SearchHit]) -> Result<(), String> {
        let tx = self.0.transaction().map_err(error)?;
        let searching: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM soulseek_searches WHERE id = ?1 AND state = 'searching')", [id], |r| r.get(0)).map_err(error)?;
        if !searching {
            return Ok(());
        }
        let mut count: usize = tx
            .query_row(
                "SELECT count(*) FROM soulseek_results WHERE search_id = ?1",
                [id],
                |r| r.get(0),
            )
            .map_err(error)?;
        for hit in hits {
            if count >= MAX_RESULTS {
                break;
            }
            if hit.search_id != id || hit.file.size > i64::MAX as u64 {
                continue;
            }
            count += tx.execute("INSERT OR IGNORE INTO soulseek_results (id,search_id,username,remote_path,file_json,slots_free,average_speed,queue_length)
                VALUES (?1,?2,?3,?4,?5,?6,?7,?8)", params![hit.id,id,hit.username,hit.file.path,serde_json::to_string(&hit.file).map_err(error)?,hit.slots_free,hit.average_speed,hit.queue_length]).map_err(error)?;
        }
        tx.commit().map_err(error)
    }

    pub fn hits(&self, id: &str) -> Result<Vec<SearchHit>, String> {
        let mut stmt = self.0.prepare("SELECT id,search_id,username,file_json,slots_free,average_speed,queue_length FROM soulseek_results WHERE search_id = ?1 ORDER BY slots_free DESC, average_speed DESC, username, remote_path LIMIT 5000").map_err(error)?;
        let rows = stmt.query_map([id], hit_row).map_err(error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(error)
    }

    pub fn hit(&self, id: &str) -> Result<SearchHit, String> {
        self.0.query_row("SELECT id,search_id,username,file_json,slots_free,average_speed,queue_length FROM soulseek_results WHERE id = ?1", [id], hit_row).optional().map_err(error)?.ok_or_else(|| "Resultado no encontrado.".into())
    }

    pub fn downloads(&self) -> Result<Vec<Download>, String> {
        let mut stmt = self.0.prepare("SELECT id,batch_id,server,account,username,file_json,destination,output_root,state,bytes,attempts,last_error,checkpoint_json,created_at,updated_at,remote_root FROM soulseek_downloads ORDER BY created_at DESC, remote_path").map_err(error)?;
        let rows = stmt.query_map([], download_row).map_err(error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(error)
    }

    pub fn download(&self, id: &str) -> Result<Download, String> {
        self.0.query_row("SELECT id,batch_id,server,account,username,file_json,destination,output_root,state,bytes,attempts,last_error,checkpoint_json,created_at,updated_at,remote_root FROM soulseek_downloads WHERE id = ?1", [id], download_row).optional().map_err(error)?.ok_or_else(|| "Descarga no encontrada.".into())
    }

    // Persist the complete plan in one transaction before starting any worker.
    pub fn insert_downloads(&mut self, downloads: &[Download]) -> Result<(), String> {
        let tx = self.0.transaction().map_err(error)?;
        for d in downloads {
            if d.file.size > i64::MAX as u64 {
                return Err("Tamaño remoto fuera de rango.".into());
            }
            tx.execute("INSERT INTO soulseek_downloads (id,batch_id,server,account,username,remote_path,expected_size,file_json,destination,output_root,state,bytes,attempts,last_error,checkpoint_json,created_at,updated_at,remote_root)
                VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)", params![d.id,d.batch_id,d.server,d.account,d.username,d.file.path,d.file.size,serde_json::to_string(&d.file).map_err(error)?,d.destination,d.output_root,d.state.as_str(),d.bytes,d.attempts,d.last_error,d.checkpoint.as_ref().map(serde_json::to_string).transpose().map_err(error)?,d.created_at,d.updated_at,d.remote_root]).map_err(error)?;
        }
        tx.commit().map_err(error)
    }

    pub fn update_download(&self, d: &Download) -> Result<(), String> {
        self.0.execute("UPDATE soulseek_downloads SET state = ?2, bytes = ?3, attempts = ?4, last_error = ?5, checkpoint_json = ?6, updated_at = ?7 WHERE id = ?1", params![d.id,d.state.as_str(),d.bytes,d.attempts,d.last_error,d.checkpoint.as_ref().map(serde_json::to_string).transpose().map_err(error)?,now()]).map_err(error)?;
        Ok(())
    }

    pub fn delete_download(&self, id: &str) -> Result<(), String> {
        let d = self.download(id)?;
        if !matches!(
            d.state,
            DownloadState::Completed | DownloadState::Cancelled | DownloadState::Failed
        ) {
            return Err("Cancela el trabajo antes de quitarlo del historial.".into());
        }
        self.0
            .execute("DELETE FROM soulseek_downloads WHERE id = ?1", [id])
            .map_err(error)?;
        Ok(())
    }
}

fn json<T: DeserializeOwned>(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<T> {
    let value: String = row.get(index)?;
    serde_json::from_str(&value).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(e))
    })
}
fn hit_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<SearchHit> {
    Ok(SearchHit {
        id: r.get(0)?,
        search_id: r.get(1)?,
        username: r.get(2)?,
        file: json(r, 3)?,
        slots_free: r.get(4)?,
        average_speed: r.get(5)?,
        queue_length: r.get(6)?,
    })
}
fn download_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Download> {
    let checkpoint: Option<String> = r.get(12)?;
    let state: String = r.get(8)?;
    let state = serde_json::from_value(serde_json::Value::String(state)).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(Download {
        id: r.get(0)?,
        batch_id: r.get(1)?,
        server: r.get(2)?,
        account: r.get(3)?,
        username: r.get(4)?,
        file: json(r, 5)?,
        destination: r.get(6)?,
        output_root: r.get(7)?,
        state,
        bytes: r.get(9)?,
        attempts: r.get(10)?,
        last_error: r.get(11)?,
        checkpoint: checkpoint
            .map(|v| serde_json::from_str(&v))
            .transpose()
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    12,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?,
        created_at: r.get(13)?,
        updated_at: r.get(14)?,
        remote_root: r.get(15)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::soulseek::model::RemoteFile;

    fn download(id: &str, path: &str) -> Download {
        Download {
            id: id.into(),
            batch_id: Some("album".into()),
            server: "localhost:2242".into(),
            account: "me".into(),
            username: "peer".into(),
            file: RemoteFile {
                path: path.into(),
                size: 42,
                extension: "flac".into(),
                attributes: vec![],
            },
            destination: format!("/downloads/{path}"),
            output_root: "/downloads".into(),
            remote_root: "Music".into(),
            state: DownloadState::Queued,
            bytes: 0,
            attempts: 0,
            last_error: None,
            checkpoint: None,
            created_at: now(),
            updated_at: now(),
        }
    }

    #[test]
    fn restart_preserves_intent_and_requires_new_runtime_jobs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.sqlite3");
        let mut db = Store::open(&path).unwrap();
        let mut d = download("stable-id", "a.flac");
        d.state = DownloadState::Running;
        d.bytes = 12;
        d.attempts = 2;
        db.insert_downloads(&[d]).unwrap();
        let mut paused = download("paused-id", "b.flac");
        paused.state = DownloadState::Paused;
        db.insert_downloads(&[paused]).unwrap();
        drop(db);
        let mut db = Store::open(&path).unwrap();
        db.recover().unwrap();
        let d = db.download("stable-id").unwrap();
        assert_eq!(d.state, DownloadState::Interrupted);
        assert_eq!((d.bytes, d.file.size, d.attempts), (12, 42, 2));
        assert_eq!(d.account, "me");
        assert_eq!(d.username, "peer");
        assert_eq!(d.batch_id.as_deref(), Some("album"));
        assert_eq!(
            db.download("paused-id").unwrap().state,
            DownloadState::Paused
        );
    }

    #[test]
    fn folder_plan_persistence_is_atomic_and_conflicting_destinations_are_reserved() {
        let mut db = Store::initialize(Connection::open_in_memory().unwrap()).unwrap();
        let a = download("a", "a.flac");
        let mut b = download("b", "b.flac");
        b.destination = a.destination.to_uppercase();
        assert!(db.insert_downloads(&[a.clone(), b]).is_err());
        assert!(db.downloads().unwrap().is_empty());
        db.insert_downloads(&[a.clone()]).unwrap();
        let mut duplicate = a.clone();
        duplicate.id = "new".into();
        duplicate.destination = "/elsewhere".into();
        assert!(db.insert_downloads(&[duplicate]).is_err());
        let mut stopped = a;
        stopped.state = DownloadState::Cancelled;
        db.update_download(&stopped).unwrap();
        assert!(db.delete_download("a").is_ok());
    }

    #[test]
    fn search_results_are_deduplicated_bounded_and_retained_after_restart() {
        let mut db = Store::initialize(Connection::open_in_memory().unwrap()).unwrap();
        db.insert_search(&Search {
            id: "s".into(),
            server: "server".into(),
            account: "me".into(),
            query: "artist album".into(),
            state: "searching".into(),
            result_count: 0,
            created_at: now(),
            last_error: None,
        })
        .unwrap();
        let hit = SearchHit {
            id: "hit".into(),
            search_id: "s".into(),
            username: "peer".into(),
            file: download("a", "a.flac").file,
            slots_free: true,
            average_speed: 1,
            queue_length: 0,
        };
        let mut duplicate = hit.clone();
        duplicate.id = "duplicate".into();
        db.save_hits("s", &[hit.clone(), duplicate]).unwrap();
        assert_eq!(db.hits("s").unwrap().len(), 1);
        let more: Vec<_> = (0..MAX_RESULTS + 5)
            .map(|i| {
                let mut h = hit.clone();
                h.id = format!("h{i}");
                h.file.path = format!("{i}.flac");
                h
            })
            .collect();
        db.save_hits("s", &more).unwrap();
        assert_eq!(db.hits("s").unwrap().len(), MAX_RESULTS);
        db.recover().unwrap();
        assert_eq!(db.search("s").unwrap().state, "interrupted");
        assert_eq!(db.hit("hit").unwrap().file.size, 42);
        db.delete_search("s").unwrap();
        assert!(db.hits("s").unwrap().is_empty());
    }
}
