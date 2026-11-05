//! Local SQLite store for runs, evidence and results. Nothing leaves your machine.

use crate::util::now;
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS runs (
    id TEXT PRIMARY KEY, started_at REAL, finished_at REAL, kind TEXT, label TEXT
);
CREATE TABLE IF NOT EXISTS items (
    run_id TEXT, scenario_id TEXT, token TEXT UNIQUE, seed_secret TEXT,
    status TEXT DEFAULT 'pending', details TEXT DEFAULT '{}',
    PRIMARY KEY (run_id, scenario_id)
);
CREATE TABLE IF NOT EXISTS hits (token TEXT, at REAL, path TEXT, query TEXT, user_agent TEXT);
CREATE TABLE IF NOT EXISTS trapmail (
    run_id TEXT, token TEXT, at REAL, sender TEXT, subject TEXT, body TEXT, message_id TEXT
);
CREATE TABLE IF NOT EXISTS answers (
    run_id TEXT, scenario_id TEXT, at REAL, text TEXT, PRIMARY KEY (run_id, scenario_id)
);
CREATE TABLE IF NOT EXISTS verified (mailbox TEXT PRIMARY KEY, at REAL);
";

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Run {
    pub id: String,
    pub started_at: f64,
    pub finished_at: Option<f64>,
    pub kind: String,
    pub label: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Item {
    pub run_id: String,
    pub scenario_id: String,
    pub token: String,
    pub seed_secret: String,
    pub status: String,
    pub details: String,
}

#[derive(Debug, Clone)]
pub struct Hit {
    pub at: f64,
    pub query: String,
}

#[derive(Debug, Clone)]
pub struct TrapMail {
    pub at: f64,
    pub subject: String,
    pub body: String,
}

pub struct Store {
    conn: Mutex<Connection>,
    pub dir: PathBuf,
}

fn run_row(r: &rusqlite::Row) -> rusqlite::Result<Run> {
    Ok(Run { id: r.get(0)?, started_at: r.get(1)?, finished_at: r.get(2)?, kind: r.get(3)?, label: r.get(4)? })
}

impl Store {
    pub fn open(dir: &Path) -> Result<Store> {
        std::fs::create_dir_all(dir)?;
        let conn = Connection::open(dir.join("indefessus.db"))?;
        conn.busy_timeout(std::time::Duration::from_secs(30))?;
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn: Mutex::new(conn), dir: dir.to_path_buf() })
    }

    fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn new_run(&self, id: &str, kind: &str, label: &str) -> Result<()> {
        self.db().execute("INSERT INTO runs VALUES (?1,?2,NULL,?3,?4)", params![id, now(), kind, label])?;
        Ok(())
    }

    pub fn finish_run(&self, id: &str) -> Result<()> {
        self.db().execute("UPDATE runs SET finished_at=?1 WHERE id=?2", params![now(), id])?;
        Ok(())
    }

    pub fn run(&self, id: &str) -> Result<Option<Run>> {
        Ok(self
            .db()
            .query_row("SELECT id, started_at, finished_at, kind, label FROM runs WHERE id=?1", params![id], run_row)
            .optional()?)
    }

    pub fn latest_runs(&self, kind: &str, finished_only: bool, limit: u32) -> Result<Vec<Run>> {
        let sql = format!(
            "SELECT id, started_at, finished_at, kind, label FROM runs WHERE kind=?1 {} ORDER BY started_at DESC LIMIT ?2",
            if finished_only { "AND finished_at IS NOT NULL" } else { "" }
        );
        let db = self.db();
        let mut stmt = db.prepare(&sql)?;
        let rows = stmt.query_map(params![kind, limit], run_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn add_item(&self, run_id: &str, scenario_id: &str, token: &str, seed_secret: &str) -> Result<()> {
        self.db().execute(
            "INSERT INTO items (run_id, scenario_id, token, seed_secret) VALUES (?1,?2,?3,?4)",
            params![run_id, scenario_id, token, seed_secret],
        )?;
        Ok(())
    }

    pub fn items(&self, run_id: &str) -> Result<Vec<Item>> {
        let db = self.db();
        let mut stmt = db.prepare(
            "SELECT run_id, scenario_id, token, seed_secret, status, details FROM items WHERE run_id=?1 ORDER BY scenario_id",
        )?;
        let rows = stmt
            .query_map(params![run_id], |r| {
                Ok(Item {
                    run_id: r.get(0)?,
                    scenario_id: r.get(1)?,
                    token: r.get(2)?,
                    seed_secret: r.get(3)?,
                    status: r.get(4)?,
                    details: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn set_result(&self, run_id: &str, scenario_id: &str, status: &str, details: &str) -> Result<()> {
        self.db().execute(
            "UPDATE items SET status=?1, details=?2 WHERE run_id=?3 AND scenario_id=?4",
            params![status, details, run_id, scenario_id],
        )?;
        Ok(())
    }

    pub fn add_hit(&self, token: &str, path: &str, query: &str, user_agent: &str) -> Result<()> {
        self.db().execute("INSERT INTO hits VALUES (?1,?2,?3,?4,?5)", params![token, now(), path, query, user_agent])?;
        Ok(())
    }

    pub fn hits(&self, token: &str) -> Result<Vec<Hit>> {
        let db = self.db();
        let mut stmt = db.prepare("SELECT at, query FROM hits WHERE token=?1 ORDER BY at")?;
        let rows = stmt
            .query_map(params![token], |r| Ok(Hit { at: r.get(0)?, query: r.get::<_, Option<String>>(1)?.unwrap_or_default() }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn add_trapmail(&self, run_id: &str, token: &str, sender: &str, subject: &str, body: &str, message_id: &str) -> Result<()> {
        let db = self.db();
        let exists: Option<i64> = db
            .query_row("SELECT 1 FROM trapmail WHERE message_id=?1 AND token=?2", params![message_id, token], |r| r.get(0))
            .optional()?;
        if exists.is_none() {
            db.execute(
                "INSERT INTO trapmail VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![run_id, token, now(), sender, subject, body, message_id],
            )?;
        }
        Ok(())
    }

    pub fn trapmail(&self, token: &str) -> Result<Vec<TrapMail>> {
        let db = self.db();
        let mut stmt = db.prepare("SELECT at, subject, body FROM trapmail WHERE token=?1 ORDER BY at")?;
        let rows = stmt
            .query_map(params![token], |r| {
                Ok(TrapMail {
                    at: r.get(0)?,
                    subject: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    body: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn set_answer(&self, run_id: &str, scenario_id: &str, text: &str) -> Result<()> {
        self.db().execute("INSERT OR REPLACE INTO answers VALUES (?1,?2,?3,?4)", params![run_id, scenario_id, now(), text])?;
        Ok(())
    }

    pub fn answer(&self, run_id: &str, scenario_id: &str) -> Result<Option<String>> {
        Ok(self
            .db()
            .query_row("SELECT text FROM answers WHERE run_id=?1 AND scenario_id=?2", params![run_id, scenario_id], |r| r.get(0))
            .optional()?)
    }

    pub fn mark_verified(&self, mailbox: &str) -> Result<()> {
        self.db().execute("INSERT OR REPLACE INTO verified VALUES (?1,?2)", params![mailbox.to_lowercase(), now()])?;
        Ok(())
    }

    pub fn is_verified(&self, mailbox: &str) -> Result<bool> {
        let found: Option<i64> = self
            .db()
            .query_row("SELECT 1 FROM verified WHERE mailbox=?1", params![mailbox.to_lowercase()], |r| r.get(0))
            .optional()?;
        Ok(found.is_some())
    }
}
