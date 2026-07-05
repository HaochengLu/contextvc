use crate::object::KnowledgeObject;
use crate::paths::ContextPaths;
use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::Path;

pub fn db_path(paths: &ContextPaths) -> std::path::PathBuf {
    paths.cache.join("index.sqlite")
}

pub fn rebuild_index(paths: &ContextPaths) -> Result<()> {
    std::fs::create_dir_all(&paths.cache)?;
    let db = db_path(paths);
    let conn = Connection::open(&db)?;
    conn.execute_batch(
        r#"
        DROP TABLE IF EXISTS objects_fts;
        DROP TABLE IF EXISTS objects;
        CREATE TABLE objects (
            id TEXT PRIMARY KEY,
            type TEXT NOT NULL,
            title TEXT NOT NULL,
            body TEXT NOT NULL,
            status TEXT NOT NULL,
            scope TEXT NOT NULL,
            confidence REAL NOT NULL,
            path TEXT NOT NULL
        );
        CREATE VIRTUAL TABLE objects_fts USING fts5(
            id UNINDEXED, title, body, scope, content='objects', content_rowid='rowid'
        );
        "#,
    )?;
    let objects = crate::object::load_all_objects(&paths.objects)?;
    for obj in objects {
        insert_object(&conn, &obj)?;
    }
    conn.execute("INSERT INTO objects_fts(objects_fts) VALUES('rebuild')", [])?;
    Ok(())
}

fn insert_object(conn: &Connection, obj: &KnowledgeObject) -> Result<()> {
    conn.execute(
        "INSERT INTO objects (id, type, title, body, status, scope, confidence, path)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            obj.frontmatter.id,
            obj.frontmatter.object_type,
            obj.frontmatter.title,
            obj.body,
            obj.frontmatter.status,
            obj.frontmatter.scope.join(","),
            obj.frontmatter.confidence,
            obj.path.to_string_lossy().as_ref(),
        ],
    )?;
    Ok(())
}

pub fn search(
    paths: &ContextPaths,
    query: &str,
    scope: Option<&str>,
    limit: usize,
) -> Result<Vec<SearchHit>> {
    let db = db_path(paths);
    if !db.exists() {
        rebuild_index(paths)?;
    }
    let conn = Connection::open(&db)?;
    let mut sql = String::from(
        "SELECT o.id, o.type, o.title, o.body, o.status, o.scope, o.confidence, bm25(objects_fts) as rank
         FROM objects_fts JOIN objects o ON objects_fts.id = o.id
         WHERE objects_fts MATCH ?1",
    );
    if scope.is_some() {
        sql.push_str(" AND o.scope LIKE ?2");
    }
    sql.push_str(" ORDER BY rank LIMIT ?3");

    let mut hits = Vec::new();
    if let Some(scope) = scope {
        let pattern = format!("%{scope}%");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![query, pattern, limit as i64], map_hit)?;
        for row in rows {
            hits.push(row?);
        }
    } else {
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![query, "", limit as i64], map_hit)?;
        for row in rows {
            hits.push(row?);
        }
    }
    Ok(hits)
}

fn map_hit(row: &rusqlite::Row<'_>) -> rusqlite::Result<SearchHit> {
    Ok(SearchHit {
        id: row.get(0)?,
        object_type: row.get(1)?,
        title: row.get(2)?,
        body: row.get(3)?,
        status: row.get(4)?,
        scope: row.get(5)?,
        confidence: row.get(6)?,
        rank: row.get(7)?,
    })
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchHit {
    pub id: String,
    pub object_type: String,
    pub title: String,
    pub body: String,
    pub status: String,
    pub scope: String,
    pub confidence: f32,
    pub rank: f64,
}

pub fn open_conn(paths: &ContextPaths) -> Result<Connection> {
    let db = db_path(paths);
    if !db.exists() {
        rebuild_index(paths)?;
    }
    Ok(Connection::open(db)?)
}

pub fn load_object_by_id(conn: &Connection, id: &str) -> Result<Option<KnowledgeObject>> {
    let path: String = conn.query_row(
        "SELECT path FROM objects WHERE id = ?1",
        params![id],
        |row| row.get(0),
    )?;
    Ok(Some(KnowledgeObject::load(Path::new(&path))?))
}
