//! `amr/index.sqlite`: an FTS5 index over plain nodes (Spike-5a). The node
//! files are the truth; the index is rebuilt from them at any time and holds
//! no sealed text. Sealed nodes get [`AmrIndex::in_memory`] once they are
//! opened, which never touches disk.

use std::path::Path;

use rusqlite::{params, Connection};

use super::schema::Node;
use super::AmrError;

/// File name under the store root.
pub const INDEX_FILE: &str = "index.sqlite";

/// One FTS5 table, `nodes_fts(id UNINDEXED, body, tags)`.
pub struct AmrIndex {
    conn: Connection,
}

impl AmrIndex {
    /// Open (or create) `index.sqlite` at `path`.
    pub fn open(path: &Path) -> Result<Self, AmrError> {
        let conn = Connection::open(path).map_err(db_err)?;
        let index = Self { conn };
        index.create()?;
        Ok(index)
    }

    /// An index that lives in memory only, for sealed nodes opened at unlock.
    pub fn in_memory(nodes: &[Node]) -> Result<Self, AmrError> {
        let conn = Connection::open_in_memory().map_err(db_err)?;
        let index = Self { conn };
        index.create()?;
        index.replace_all(nodes)?;
        Ok(index)
    }

    fn create(&self) -> Result<(), AmrError> {
        self.conn
            .execute_batch("CREATE VIRTUAL TABLE IF NOT EXISTS nodes_fts USING fts5(id UNINDEXED, body, tags);")
            .map_err(db_err)
    }

    /// Drop every row and index `nodes` instead.
    pub fn replace_all(&self, nodes: &[Node]) -> Result<(), AmrError> {
        self.conn.execute("DELETE FROM nodes_fts", []).map_err(db_err)?;
        for node in nodes {
            self.add(node)?;
        }
        Ok(())
    }

    pub fn add(&self, node: &Node) -> Result<(), AmrError> {
        self.remove(&node.id)?;
        self.conn
            .execute(
                "INSERT INTO nodes_fts (id, body, tags) VALUES (?1, ?2, ?3)",
                params![node.id, node.body, node.tags.join(" ")],
            )
            .map_err(db_err)?;
        Ok(())
    }

    pub fn remove(&self, id: &str) -> Result<(), AmrError> {
        self.conn
            .execute("DELETE FROM nodes_fts WHERE id = ?1", params![id])
            .map_err(db_err)?;
        Ok(())
    }

    /// Every indexed id, sorted.
    pub fn ids(&self) -> Result<Vec<String>, AmrError> {
        let mut stmt = self.conn.prepare("SELECT id FROM nodes_fts ORDER BY id").map_err(db_err)?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0)).map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    /// Ids whose body or tags hold every word of `query` (prefix match per
    /// word), best match first. Words are quoted, so FTS syntax in the query
    /// is plain text.
    pub fn search(&self, query: &str) -> Result<Vec<String>, AmrError> {
        let terms: Vec<String> = query
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(|w| format!("\"{}\"*", w.to_lowercase()))
            .collect();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM nodes_fts WHERE nodes_fts MATCH ?1 ORDER BY bm25(nodes_fts), id")
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![terms.join(" ")], |row| row.get::<_, String>(0))
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }
}

fn db_err(err: rusqlite::Error) -> AmrError {
    AmrError::Io(format!("index: {err}"))
}
