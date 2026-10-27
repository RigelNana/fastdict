use crate::model::*;
use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
};
pub struct Store {
    pub path: PathBuf,
    connection: Mutex<Connection>,
    snapshot: RwLock<Arc<Snapshot>>,
}
impl Store {
    pub fn open(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root)?;
        let current = root.join("fastdict.sqlite3");
        let legacy = root.join("mydict.sqlite3");
        // Existing installations keep their database and absolute resource paths intact.
        let path = if !current.exists() && legacy.is_file() { legacy } else { current };
        let c = Connection::open(&path)?;
        c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; CREATE TABLE IF NOT EXISTS configuration(id INTEGER PRIMARY KEY CHECK(id=1),data TEXT NOT NULL); CREATE TABLE IF NOT EXISTS logs(id TEXT PRIMARY KEY,at TEXT NOT NULL,level TEXT NOT NULL,scope TEXT NOT NULL,message TEXT NOT NULL); CREATE TABLE IF NOT EXISTS headwords(dictionary_id TEXT NOT NULL,word TEXT NOT NULL); CREATE INDEX IF NOT EXISTS headwords_dictionary ON headwords(dictionary_id); CREATE VIRTUAL TABLE IF NOT EXISTS fulltext USING fts5(dictionary_id UNINDEXED, record_id UNINDEXED, word UNINDEXED, body, tokenize='unicode61');")?;
        c.execute(
            "INSERT OR IGNORE INTO configuration VALUES(1,?1)",
            [serde_json::to_string(&Snapshot::default())?],
        )?;
        let data: String = c.query_row("SELECT data FROM configuration WHERE id=1", [], |r| {
            r.get(0)
        })?;
        let snapshot = serde_json::from_str(&data).context("Invalid configuration")?;
        Ok(Self {
            path,
            connection: Mutex::new(c),
            snapshot: RwLock::new(Arc::new(snapshot)),
        })
    }
    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.snapshot.read().unwrap().clone()
    }
    pub fn load(&self) -> Result<Snapshot> {
        Ok(self.snapshot().as_ref().clone())
    }
    pub fn update<T>(&self, f: impl FnOnce(&mut Snapshot) -> Result<T>) -> Result<T> {
        let mut c = self.connection.lock().unwrap();
        let mut snapshot = self.snapshot().as_ref().clone();
        let result = f(&mut snapshot)?;
        let tx = c.transaction()?;
        tx.execute(
            "UPDATE configuration SET data=?1 WHERE id=1",
            [serde_json::to_string(&snapshot)?],
        )?;
        tx.commit()?;
        *self.snapshot.write().unwrap() = Arc::new(snapshot);
        Ok(result)
    }
    pub fn log(&self, level: &str, scope: &str, message: &str) {
        if let Ok(c) = self.connection.lock() {
            let _ = c.execute(
                "INSERT INTO logs VALUES(?1,?2,?3,?4,?5)",
                params![id(), now(), level, scope, message],
            );
            let _=c.execute("DELETE FROM logs WHERE id NOT IN (SELECT id FROM logs ORDER BY at DESC LIMIT 2000)",[]);
        }
    }
    pub fn logs(&self, limit: usize) -> Result<Vec<LogItem>> {
        let c = self.connection.lock().unwrap();
        let mut q =
            c.prepare("SELECT id,at,level,scope,message FROM logs ORDER BY at DESC LIMIT ?1")?;
        let rows = q.query_map([limit], |r| {
            Ok(LogItem {
                id: r.get(0)?,
                at: r.get(1)?,
                level: r.get(2)?,
                scope: r.get(3)?,
                message: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
    pub fn clear_logs(&self) -> Result<()> {
        self.connection
            .lock()
            .unwrap()
            .execute("DELETE FROM logs", [])?;
        Ok(())
    }
    pub fn words(&self) -> Result<Vec<(String, String)>> {
        let c = self.connection.lock().unwrap();
        let mut q = c.prepare("SELECT dictionary_id,word FROM headwords")?;
        Ok(q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn set_words(&self, dictionary: &str, words: &[String]) -> Result<()> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute("DELETE FROM headwords WHERE dictionary_id=?1", [dictionary])?;
        {
            let mut q = tx.prepare("INSERT INTO headwords VALUES(?1,?2)")?;
            for word in words {
                q.execute(params![dictionary, word])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
    pub fn clear_derived(
        &self,
        dictionary: Option<&str>,
        headwords: bool,
        fulltext: bool,
    ) -> Result<()> {
        let c = self.connection.lock().unwrap();
        for table in [
            if headwords { Some("headwords") } else { None },
            if fulltext { Some("fulltext") } else { None },
        ]
        .into_iter()
        .flatten()
        {
            if let Some(id) = dictionary {
                c.execute(&format!("DELETE FROM {table} WHERE dictionary_id=?1"), [id])?;
            } else {
                c.execute(&format!("DELETE FROM {table}"), [])?;
            }
        }
        Ok(())
    }
}

impl Store {
    pub fn restore_snapshot(
        &self,
        snapshot: Snapshot,
        catalogs: &[(String, Vec<String>)],
    ) -> Result<()> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute(
            "UPDATE configuration SET data=?1 WHERE id=1",
            [serde_json::to_string(&snapshot)?],
        )?;
        tx.execute("DELETE FROM headwords", [])?;
        tx.execute("DELETE FROM fulltext", [])?;
        {
            let mut insert = tx.prepare("INSERT INTO headwords VALUES(?1,?2)")?;
            for (id, words) in catalogs {
                for word in words {
                    insert.execute(params![id, word])?;
                }
            }
        }
        tx.commit()?;
        *self.snapshot.write().unwrap() = Arc::new(snapshot);
        Ok(())
    }
    pub fn commit_import(
        &self,
        dictionaries: &[Dictionary],
        catalogs: &[(String, Vec<String>)],
    ) -> Result<()> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction()?;
        let mut snapshot = self.snapshot().as_ref().clone();
        anyhow::ensure!(
            snapshot.dictionaries.len() + dictionaries.len() <= 1000,
            "Dictionary limit is 1000"
        );
        snapshot.dictionaries.extend_from_slice(dictionaries);
        tx.execute(
            "UPDATE configuration SET data=?1 WHERE id=1",
            [serde_json::to_string(&snapshot)?],
        )?;
        {
            let mut insert = tx.prepare("INSERT INTO headwords VALUES(?1,?2)")?;
            for (id, words) in catalogs {
                for word in words {
                    insert.execute(params![id, word])?;
                }
            }
        }
        tx.commit()?;
        *self.snapshot.write().unwrap() = Arc::new(snapshot);
        Ok(())
    }
    pub fn update_dictionary(&self, dictionary: &Dictionary, clear_fulltext: bool) -> Result<()> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction()?;
        let mut snapshot = self.snapshot().as_ref().clone();
        let previous = snapshot
            .dictionaries
            .iter_mut()
            .find(|d| d.id == dictionary.id)
            .context("Dictionary removed")?;
        let index_status = previous.index_status.clone();
        *previous = dictionary.clone();
        if !clear_fulltext {
            previous.index_status = index_status;
        }
        tx.execute(
            "UPDATE configuration SET data=?1 WHERE id=1",
            [serde_json::to_string(&snapshot)?],
        )?;
        if clear_fulltext {
            tx.execute(
                "DELETE FROM fulltext WHERE dictionary_id=?1",
                [&dictionary.id],
            )?;
        }
        tx.commit()?;
        *self.snapshot.write().unwrap() = Arc::new(snapshot);
        Ok(())
    }
}
